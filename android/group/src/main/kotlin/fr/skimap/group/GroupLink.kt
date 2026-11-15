package fr.skimap.group

import android.content.Context
import android.location.Location
import android.os.BatteryManager
import android.os.Handler
import android.util.Log
import fr.skimap.tracking.AlertKind
import fr.skimap.tracking.Batch
import fr.skimap.tracking.OutingExtension
import fr.skimap.tracking.Phase
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** The group during an outing, for the map and the group screen. */
data class GroupLive(
    val members: List<Member>,
    /** Whether the radio works (Bluetooth on and permitted). */
    val radio: Boolean,
    /** Frames that gave a newer position since the start. */
    val heard: Int,
)

/** The group as the screens see it: null outside an outing with a group. */
object Groups {
    private val mutable = MutableStateFlow<GroupLive?>(null)
    val live: StateFlow<GroupLive?> = mutable.asStateFlow()

    internal fun publish(g: GroupLive?) {
        mutable.value = g
    }
}

/**
 * The group mode inside the location service (N2): at each GPS batch this phone's position
 * goes to the core with the frames heard since the last one, and the frames to advertise,
 * the alerts and the members come back; frames heard between two GPS batches (the scan's
 * own batches) are handled when they come. Everything on the service's thread; no
 * service, wakelock or timer of its own. Nothing runs when this phone has no group.
 */
class GroupLink(
    private val backend: GroupBackend,
    private val store: GroupStore,
    private val planDir: String?,
    private val radioOf: (Context, Handler) -> GroupRadio = { c, h -> BleGroupRadio(c, h, backend.serviceUuid) },
    private val clock: () -> Long = System::currentTimeMillis,
) : OutingExtension {
    private var session: GroupSessionCore? = null
    private var radio: GroupRadio? = null
    private var output: OutingExtension.Output? = null
    private var battery: BatteryManager? = null
    private var prefixes: List<ByteArray> = emptyList()
    private var offRoute = false
    private var batchDelayMs = DEFAULT_BATCH_MS
    private var heard = 0

    /** Alerts told (tests). */
    val told: MutableList<GroupAlert> = java.util.Collections.synchronizedList(ArrayList())

    override fun start(
        context: Context,
        handler: Handler,
        output: OutingExtension.Output,
    ) {
        val j = store.joined() ?: return
        val s =
            runCatching { backend.open(j.qrText, j.me, planDir) }.getOrElse {
                Log.w(TAG, "group: cannot open", it)
                return
            }
        session = s
        this.output = output
        battery = context.getSystemService(BatteryManager::class.java)
        radio = radioOf(context, handler)
        active = this
        Log.i(TAG, "group: member ${j.me}, radio ${if (radio?.available == true) "on" else "off"}")
        apply(s.tick(clock(), null, emptyList()))
    }

    override fun onBatch(
        batch: Batch,
        fix: Location?,
    ) {
        val s = session ?: return
        for (a in batch.alerts) {
            if (a == AlertKind.OFF_ROUTE) offRoute = true
            if (a == AlertKind.BACK_ON_ROUTE) offRoute = false
        }
        batch.gps?.let { batchDelayMs = it.maxDelayMs.coerceIn(MIN_BATCH_MS, MAX_BATCH_MS) }
        if (fix == null) return
        val g = batch.guidance
        val own =
            OwnFix(
                tMs = fix.time,
                lat = fix.latitude,
                lon = fix.longitude,
                phase =
                    when (g.phase) {
                        Phase.PAUSED -> OwnPhase.PAUSED
                        Phase.REFUGE -> OwnPhase.REFUGE
                        else -> OwnPhase.MOVING
                    },
                offRoute = offRoute,
                // Progress along the plan only on the plan itself (not a plan B or « ramène-moi »).
                progressM = g.progressM.takeIf { g.route == "plan" },
                batteryLow = batteryLow(),
            )
        apply(s.tick(clock(), own, emptyList()))
    }

    /** Frames heard (the radio's batches; injected by the instrumented test). */
    fun receive(frames: List<ByteArray>) {
        val s = session ?: return
        apply(s.tick(clock(), null, frames))
    }

    private fun batteryLow(): Boolean {
        val pct = battery?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY) ?: return false
        return pct in 1..LOW_BATTERY_PCT
    }

    private fun apply(t: Tick) {
        val r = radio
        heard += t.newPositions
        if (r != null) {
            if (!samePrefixes(t.prefixes)) {
                prefixes = t.prefixes
                r.scan(prefixes, batchDelayMs, ::receive)
            }
            r.advertise(t.ownFrame, t.relayFrame)
        }
        for (a in t.alerts) {
            told += a
            output?.alert(a.text)
        }
        Groups.publish(GroupLive(t.members, r?.available == true, heard))
    }

    private fun samePrefixes(p: List<ByteArray>) = p.size == prefixes.size && p.indices.all { p[it].contentEquals(prefixes[it]) }

    override fun stop() {
        radio?.stop()
        radio = null
        session?.close()
        session = null
        prefixes = emptyList()
        if (active === this) active = null
        Groups.publish(null)
    }

    companion object {
        private const val TAG = "skimap"
        private const val DEFAULT_BATCH_MS = 10_000L
        private const val MIN_BATCH_MS = 5_000L
        private const val MAX_BATCH_MS = 60_000L
        private const val LOW_BATTERY_PCT = 15

        /** The link of the outing running (instrumented tests inject frames into it). */
        @Volatile
        var active: GroupLink? = null
            private set
    }
}
