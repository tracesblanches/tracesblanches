package fr.skimap.tracking

import android.annotation.SuppressLint
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener2
import android.hardware.SensorManager
import android.hardware.TriggerEvent
import android.hardware.TriggerEventListener
import android.location.Location
import android.location.LocationManager
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.edit
import androidx.core.location.LocationListenerCompat
import androidx.core.location.LocationManagerCompat
import androidx.core.os.HandlerCompat
import java.io.File

/**
 * The one location service of an outing (K4, K8, E1; report 09): a foreground service of
 * type `location`, started by « Valider la journée » (a user gesture: it keeps the
 * position and the sound while the screen is off), never restarted from the background.
 *
 * Everything runs on one thread that sleeps between GPS batches: no wakelock, no timer,
 * no polling. The fixes arrive in batches (`setMaxUpdateDelayMillis`) and go to the core in
 * one call per batch ([TrackingCore.pushFixes]); the core decides the GPS mode (pocket,
 * approach, deviation, interaction, pause) and the service only applies it: interval and
 * batch delay, GNSS off and the significant motion sensor armed during a pause. The
 * screen state goes to the core (interaction mode) and to the physical controls (volume
 * keys when the screen is off); at screen on the pending batch is flushed so that the
 * banner is fresh.
 *
 * The app subclasses it with the core ([openCore]) and declares it in its manifest. One
 * class for the lifecycle callbacks, the platform listeners and the commands.
 */
@Suppress("TooManyFunctions")
abstract class TrackingService : Service() {
    /** The core's tracker of the plan in [planDir]. */
    protected abstract fun openCore(planDir: String): TrackingCore

    /** Called on the service's thread at the end of the outing (recap, GPX, what the group learned). */
    protected open fun onEnded(
        planDir: String,
        recap: Recap?,
        gpx: String?,
    ) = Unit

    /**
     * Called on the service's thread when the skier chose the profile the core proposed
     * (X14): [profileJson] is the core's `GroupProfile`; the app makes it the group of the
     * day (what the outing teaches goes to it).
     */
    protected open fun onProfileChosen(profileJson: String) = Unit

    private lateinit var thread: HandlerThread
    private lateinit var handler: Handler
    private lateinit var executor: java.util.concurrent.Executor
    private lateinit var announcer: Announcer
    private lateinit var haptics: Haptics
    private lateinit var notifier: GuidanceNotifier
    private var controls: PhysicalControls? = null
    private var core: TrackingCore? = null
    private var planDir: String? = null
    private var provider: String = LocationManager.GPS_PROVIDER
    private var applied: GpsRequest? = null
    private var listening = false
    private var guidance: Guidance? = null
    private var lastSaid: String? = null
    private var ended = false
    private var screenRegistered = false
    private var screenOn = true
    private var recap: Recap? = null
    private var recapAt = 0L
    private var lastFixT = 0L

    /** Replay: the inputs come from the test, not from the platform (its fixes would carry today's time). */
    private var replaying = false

    /** X10: the step counter in 10 s reports, and the batch of fixes waiting for its flush. */
    private val steps = StepReports()
    private var stepsOn = false
    private var waiting: List<Location>? = null

    /** K6: the short sounds, read when the outing starts. */
    private var earconSettings = EarconSettings(true, EarconSettings.DEFAULT_VOLUME)

    /** Counters of the outing (logged at the end; read by the instrumented tests). */
    class Stats {
        @Volatile var batches = 0

        @Volatile var fixes = 0

        @Volatile var requests = 0

        @Volatile var motionArmed = 0

        /** Announcements finished (said or failed), said with the pack's clips, with the Android voice. */
        @Volatile var voiced = 0

        @Volatile var voicedClips = 0

        @Volatile var voicedTts = 0

        /** Notifications posted (only when their text changes). */
        @Volatile var notified = 0

        /** Step reports given to the core (X10), and the short sounds asked (K6). */
        @Volatile var stepReports = 0

        @Volatile var earcons = 0

        val gpsLog: MutableList<GpsRequest> = java.util.Collections.synchronizedList(ArrayList())
        val said: MutableList<Utterance> = java.util.Collections.synchronizedList(ArrayList())
        val alerts: MutableList<AlertKind> = java.util.Collections.synchronizedList(ArrayList())
        val switches: MutableList<RouteSwitch> = java.util.Collections.synchronizedList(ArrayList())
        val proposals: MutableList<Proposal> = java.util.Collections.synchronizedList(ArrayList())
        val session: MutableList<String> = java.util.Collections.synchronizedList(ArrayList())
        val profileProposals: MutableList<ProfileProposal> = java.util.Collections.synchronizedList(ArrayList())

        /** The profile chosen during the outing (X14), JSON. */
        @Volatile var profileChosen: String? = null
    }

    val stats = Stats()

    private val listener =
        object : LocationListenerCompat {
            override fun onLocationChanged(location: Location) = fixes(listOf(location))

            override fun onLocationChanged(locations: MutableList<Location>) = fixes(locations)

            override fun onProviderDisabled(provider: String) = Unit

            override fun onProviderEnabled(provider: String) = Unit
        }

    private val motion =
        object : TriggerEventListener() {
            override fun onTrigger(event: TriggerEvent?) {
                handler.post { core?.let { handle(it.activity(System.currentTimeMillis(), Activity.SIGNIFICANT_MOTION)) } }
            }
        }

    /**
     * Android's step counter (X10): a non-wake-up sensor batched in its FIFO (it never wakes
     * the phone); its events come when the phone is awake for a batch of fixes, which asks
     * for them ([SensorManager.flush]) and goes to the core with them once flushed.
     */
    private val stepListener =
        object : SensorEventListener2 {
            override fun onSensorChanged(event: SensorEvent) {
                // Sensor time is the elapsed realtime clock: to the wall clock of the fixes.
                val ageMs = (SystemClock.elapsedRealtimeNanos() - event.timestamp) / NANOS_PER_MS
                steps.sample(System.currentTimeMillis() - ageMs, event.values[0].toLong())
            }

            override fun onAccuracyChanged(
                sensor: Sensor?,
                accuracy: Int,
            ) = Unit

            override fun onFlushCompleted(sensor: Sensor?) {
                steps.close(System.currentTimeMillis())
                waiting?.let {
                    waiting = null
                    push(it)
                }
            }
        }

    private val screen =
        object : BroadcastReceiver() {
            override fun onReceive(
                context: Context,
                intent: Intent,
            ) {
                val on = intent.action == Intent.ACTION_SCREEN_ON
                screenOn = on
                controls?.screen(on)
                val c = core ?: return
                handle(c.screen(System.currentTimeMillis(), on))
                if (on && listening && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    // The fixes waiting in the chip's batch, now: the screen shows the present.
                    locationManager()?.requestFlush(provider, listener, 0)
                }
            }
        }

    /** The commands of the screens and of the notification's actions, run on the service's thread. */
    private val commands: Map<String, () -> Unit> =
        mapOf(
            Tracking.ACTION_STOP to { end(null, stopped = true) },
            Tracking.ACTION_ACCEPT to { core?.let { handle(it.answer(System.currentTimeMillis(), true)) } },
            Tracking.ACTION_DECLINE to { core?.let { handle(it.answer(System.currentTimeMillis(), false)) } },
            Tracking.ACTION_REFUSE_SWITCH to { core?.let { handle(it.refuse(System.currentTimeMillis())) } },
            Tracking.ACTION_BACK to { toggleBack() },
            Tracking.ACTION_SAY to { act(ButtonAction.BOTH) },
            Tracking.ACTION_PROFILE_ACCEPT to { core?.let { handle(it.profileAnswer(System.currentTimeMillis(), true)) } },
            Tracking.ACTION_PROFILE_DECLINE to { core?.let { handle(it.profileAnswer(System.currentTimeMillis(), false)) } },
        )

    /** Local binding: the app's screens and the instrumented tests reach the service. */
    inner class Local : Binder() {
        val service: TrackingService get() = this@TrackingService
    }

    override fun onBind(intent: Intent?): IBinder = Local()

    override fun onCreate() {
        super.onCreate()
        thread = HandlerThread("skimap-tracking", Process.THREAD_PRIORITY_BACKGROUND).also { it.start() }
        handler = HandlerCompat.createAsync(thread.looper)
        executor = java.util.concurrent.Executor { handler.post(it) }
        announcer = Announcer(this)
        announcer.onDone = { _, _ ->
            stats.voiced++
            stats.voicedClips = announcer.played
            stats.voicedTts = announcer.spokenByTts
        }
        haptics = Haptics(this)
        notifier = GuidanceNotifier(this, javaClass)
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        val action = intent?.action
        if (action == Tracking.ACTION_START) {
            val dir = intent.getStringExtra(Tracking.EXTRA_PLAN_DIR)
            foreground()
            if (dir != null && core == null) {
                val wanted = intent.getStringExtra(Tracking.EXTRA_PROVIDER)
                val replay = intent.getBooleanExtra(Tracking.EXTRA_REPLAY, false)
                handler.post { open(dir, wanted, replay) }
            }
        } else if (action == null) {
            if (core == null) stopSelf(startId)
        } else {
            commands[action]?.let { handler.post(it) }
        }
        // Never restarted by the system: from the background it would have neither the
        // position nor the sound (report 09). The app offers to resume instead.
        return START_NOT_STICKY
    }

    private fun foreground() {
        ServiceCompat.startForeground(
            this,
            GuidanceNotifier.ID,
            notifier.build(guidance, starting = guidance == null),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION,
        )
    }

    private fun locationManager(): LocationManager? = getSystemService(LocationManager::class.java)

    private fun open(
        dir: String,
        wanted: String?,
        replay: Boolean,
    ) {
        // Whatever the core says is wrong with the plan (its own exception type, from the app).
        val c =
            runCatching { openCore(dir) }.getOrElse {
                Log.e(TAG, "tracking: cannot open $dir", it)
                Tracking.publish(TrackingState.Ended(dir, null, null))
                stopSelf()
                return
            }
        core = c
        planDir = dir
        ended = false
        recap = null
        replaying = replay
        getSharedPreferences(Tracking.PREFS, MODE_PRIVATE).edit { putString(Tracking.KEY_ACTIVE, dir) }
        val lm = locationManager()
        provider =
            when {
                wanted == "gps" -> {
                    LocationManager.GPS_PROVIDER
                }

                lm != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && lm.hasProvider(LocationManager.FUSED_PROVIDER) -> {
                    LocationManager.FUSED_PROVIDER
                }

                else -> {
                    LocationManager.GPS_PROVIDER
                }
            }
        Log.i(TAG, "tracking: $dir, provider $provider")
        announcer.warmUp()
        controls = PhysicalControls(this, handler, ::act)
        val interactive = getSystemService(PowerManager::class.java)?.isInteractive ?: true
        screenOn = interactive
        controls?.screen(interactive)
        earconSettings = Tracking.earcons(this)
        apply(GpsPolicy.START)
        if (replay) {
            publish()
            return
        }
        startSteps()
        val f =
            IntentFilter().apply {
                addAction(Intent.ACTION_SCREEN_ON)
                addAction(Intent.ACTION_SCREEN_OFF)
            }
        registerReceiver(screen, f, null, handler)
        screenRegistered = true
        handle(c.screen(System.currentTimeMillis(), interactive))
    }

    private fun publish() {
        val dir = planDir ?: return
        val g = guidance ?: core?.guidance() ?: return
        guidance = g
        Tracking.publish(TrackingState.Running(dir, g, applied, lastSaid))
    }

    /** The step counter, batched without waking the phone (X10); absent on some phones. */
    private fun startSteps() {
        val sm = getSystemService(SensorManager::class.java) ?: return
        val sensor = sm.getDefaultSensor(Sensor.TYPE_STEP_COUNTER) ?: return
        stepsOn = sm.registerListener(stepListener, sensor, SensorManager.SENSOR_DELAY_NORMAL, STEP_LATENCY_US, handler)
        Log.i(TAG, "tracking: step counter ${if (stepsOn) "on" else "refused"} (wake-up ${sensor.isWakeUpSensor})")
    }

    /** A batch of fixes from the platform (or from a replay). */
    fun fixes(locations: List<Location>) {
        // After the end, fixes still in flight are dropped (the outing is over).
        if (core == null || locations.isEmpty() || ended) return
        // A batch still waiting for the steps (no flush answer): it goes first.
        waiting?.let {
            waiting = null
            push(it)
        }
        val sm = getSystemService(SensorManager::class.java)
        if (stepsOn && sm != null && sm.flush(stepListener)) {
            waiting = locations
        } else {
            push(locations)
        }
    }

    /** One call to the core: the batch and the step reports closed by then. */
    private fun push(locations: List<Location>) {
        val c = core ?: return
        if (ended) return
        lastFixT = locations.last().time
        stats.batches++
        stats.fixes += locations.size
        val reports = steps.drain()
        stats.stepReports += reports.size
        val packed = FixPacker.pack(locations)
        handle(if (reports.isEmpty()) c.pushFixes(packed) else c.pushBatch(packed, reports))
    }

    /** A batch of fixes and the step reports over the same time, as recorded (replays of recorded inputs). */
    fun fixes(
        locations: List<Location>,
        reports: List<StepReport>,
    ) {
        val c = core ?: return
        if (locations.isEmpty() || ended) return
        lastFixT = locations.last().time
        stats.batches++
        stats.fixes += locations.size
        stats.stepReports += reports.size
        handle(c.pushBatch(FixPacker.pack(locations), reports))
    }

    /** Test hook: an event of the step counter ([total] steps since boot at [tMs]). */
    fun stepEvent(
        tMs: Long,
        total: Long,
    ) {
        steps.sample(tMs, total)
    }

    /** Test hook: the step counter flushed at [tMs] (every event up to then is known). */
    fun stepsFlushed(tMs: Long) {
        steps.close(tMs)
    }

    /** Short sound of K6 when on. */
    private fun earcon(e: Earcon?) {
        if (e == null || !earconSettings.enabled) return
        stats.earcons++
        announcer.earcon(e, earconSettings.volume)
    }

    /** Location provider used: `fused` where the platform has it, else `gps`. */
    val providerName: String get() = provider

    /**
     * Gives [block]'s call to the core on the service's thread and handles its result as
     * any other (replays of recorded inputs, instrumented tests).
     */
    fun inject(block: (TrackingCore) -> Batch) {
        handler.post { core?.let { handle(block(it)) } }
    }

    /** The service's thread (tests wait on it). */
    val looper: android.os.Looper get() = thread.looper

    /** Runs [block] on the service's thread (tests, replay). */
    fun post(block: () -> Unit) {
        handler.post(block)
    }

    private fun handle(b: Batch) {
        if (ended) return
        // Off the route or back on it: its sound before the words (K6).
        for (a in b.alerts) {
            stats.alerts += a
            haptics.play(HapticPatterns.alert(a))
            earcon(EarconSynth.alert(a))
        }
        for (u in b.utterances) {
            stats.said += u
            lastSaid = u.text
            if ("turn" in u.kinds) {
                // Left in the left ear, right in the right one, just before the words.
                earcon(EarconSynth.turn(b.guidance.nextTurn))
                HapticPatterns.turn(b.guidance.nextTurn)?.let(haptics::play)
            }
            announcer.say(u)
        }
        proposals(b)
        stats.proposals += b.proposals
        stats.switches += b.switches
        stats.session += b.session
        b.gps?.let(::apply)
        guidance = b.guidance
        if (notifier.update(b.guidance, screenOn)) stats.notified++
        planDir?.let { Tracking.publish(TrackingState.Running(it, b.guidance, applied, lastSaid)) }
        if (b.recap != null && recap == null) {
            // The last words (« Sortie terminée. ») come a few seconds after the end (the
            // speech groups for 8 s): the service ends once they are said, or 30 s later.
            recap = b.recap
            recapAt = lastFixT
            handler.postDelayed({ end(recap, stopped = false) }, END_GRACE_MS)
        }
        val r = recap
        if (r != null && (b.utterances.any { "finished" in it.kinds } || lastFixT - recapAt >= END_GRACE_MS)) {
            end(r, stopped = false)
        }
    }

    /** Proposals of a shorter way (C11) or of another profile (X14): felt; a profile chosen goes to the app. */
    private fun proposals(b: Batch) {
        if (b.proposals.isNotEmpty() || b.profileProposals.isNotEmpty()) haptics.play(HapticPatterns.PROPOSAL)
        stats.profileProposals += b.profileProposals
        b.profileChosen?.let { chosen ->
            stats.profileChosen = chosen
            onProfileChosen(chosen)
        }
    }

    /** Applies the core's GPS request (only what changed: no yoyo of the chip). */
    @SuppressLint("MissingPermission")
    private fun apply(r: GpsRequest) {
        stats.gpsLog += r
        val lm = locationManager()
        if (lm != null && !replaying && GpsPolicy.changes(applied, r)) {
            try {
                if (listening) LocationManagerCompat.removeUpdates(lm, listener)
                LocationManagerCompat.requestLocationUpdates(
                    lm,
                    if (r.gnss) provider else LocationManager.PASSIVE_PROVIDER,
                    GpsPolicy.locationRequest(r),
                    executor,
                    listener,
                )
                listening = true
                stats.requests++
            } catch (e: SecurityException) {
                Log.e(TAG, "tracking: no location permission", e)
            } catch (e: IllegalArgumentException) {
                Log.e(TAG, "tracking: provider $provider", e)
            }
        }
        val sm = getSystemService(SensorManager::class.java)
        val sensor = sm?.getDefaultSensor(Sensor.TYPE_SIGNIFICANT_MOTION)
        if (sensor != null && !replaying) {
            if (r.significantMotion && applied?.significantMotion != true) {
                sm.requestTriggerSensor(motion, sensor)
                stats.motionArmed++
            } else if (!r.significantMotion && applied?.significantMotion == true) {
                sm.cancelTriggerSensor(motion, sensor)
            }
        }
        applied = r
    }

    private fun act(a: ButtonAction) {
        val c = core ?: return
        val now = System.currentTimeMillis()
        Log.i(TAG, "button: $a")
        when (a) {
            ButtonAction.NEXT_DIRECTION -> {
                say(c.sayNext(now))
            }

            ButtonAction.TIME_LEFT -> {
                say(c.sayRemaining(now))
            }

            ButtonAction.BOTH -> {
                say(c.sayNext(now))
                say(c.sayRemaining(now))
            }

            ButtonAction.BRING_ME_BACK -> {
                toggleBack()
            }
        }
    }

    /** Says what a button asked for. */
    private fun say(u: Utterance?) {
        if (u == null) return
        stats.said += u
        lastSaid = u.text
        announcer.say(u)
    }

    private fun toggleBack() {
        val c = core ?: return
        handle(if (guidance?.route == "back") c.backStop() else c.backStart(System.currentTimeMillis()))
    }

    private fun end(
        recap: Recap?,
        stopped: Boolean,
    ) {
        if (ended) return
        ended = true
        val c = core
        val dir = planDir
        var gpx: String? = null
        if (c != null && dir != null) {
            runCatching {
                gpx = c.writeGpx()
                File(dir, OBSERVATION_FILE).writeText(c.observation())
            }.onFailure { Log.w(TAG, "tracking: end files", it) }
            Log.i(
                TAG,
                "tracking ended (${if (stopped) "stopped" else "finished"}): ${stats.fixes} fixes in ${stats.batches} batches, " +
                    "${stats.requests} GPS requests, ${stats.said.size} said, ${stats.notified} notifications",
            )
            onEnded(dir, recap, gpx)
        }
        getSharedPreferences(Tracking.PREFS, MODE_PRIVATE).edit { remove(Tracking.KEY_ACTIVE) }
        Tracking.publish(TrackingState.Ended(dir ?: "", recap, gpx))
        stopListening()
        // The last words are said before the service goes (sound needs the foreground service).
        announcer.afterQueue { handler.post { stopSelf() } }
    }

    @SuppressLint("MissingPermission")
    private fun stopListening() {
        val lm = locationManager()
        if (listening && lm != null) LocationManagerCompat.removeUpdates(lm, listener)
        listening = false
        getSystemService(SensorManager::class.java)?.let { sm ->
            sm.getDefaultSensor(Sensor.TYPE_SIGNIFICANT_MOTION)?.let { sm.cancelTriggerSensor(motion, it) }
            if (stepsOn) sm.unregisterListener(stepListener)
        }
        stepsOn = false
        waiting = null
    }

    override fun onDestroy() {
        handler.post {
            stopListening()
            if (screenRegistered) unregisterReceiver(screen)
            screenRegistered = false
            controls?.close()
            controls = null
            core?.close()
            core = null
        }
        announcer.close()
        thread.quitSafely()
        super.onDestroy()
    }

    companion object {
        const val TAG = "skimap"

        /** What the outing told about the group (C11), written at the end. */
        const val OBSERVATION_FILE = "observation.json"

        /** After the recap, the last words are waited for this long at most, milliseconds. */
        const val END_GRACE_MS = 30_000L

        /** The step counter's FIFO is read at most this late (it never wakes the phone), microseconds. */
        const val STEP_LATENCY_US = 60_000_000

        private const val NANOS_PER_MS = 1_000_000L
    }
}
