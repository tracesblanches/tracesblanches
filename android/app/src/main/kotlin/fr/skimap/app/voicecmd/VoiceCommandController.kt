package fr.skimap.app.voicecmd

import android.content.Context
import android.os.BatteryManager
import android.util.Log
import fr.skimap.app.guidance.RustTrackingCore
import fr.skimap.core.VoiceAnswers
import fr.skimap.tracking.Announcer
import fr.skimap.tracking.Batch
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState
import fr.skimap.voicecmd.CommandLang
import fr.skimap.voicecmd.EngineLoader
import fr.skimap.voicecmd.FrameSource
import fr.skimap.voicecmd.Heard
import fr.skimap.voicecmd.MicSource
import fr.skimap.voicecmd.ModelStore
import fr.skimap.voicecmd.VoiceCommands
import java.io.File
import java.util.Locale
import kotlin.concurrent.thread

/**
 * Voice commands in the app (V3 #66, VO5 #87): the optional model in `files/voicecmd`
 * ([install], 642 MiB, never in the APK), and one command per gesture ([ask]: the
 * microphone button, the headset button held), answered on the location service's
 * thread with the clips of the pack (or by the Android voice when there is no outing).
 */
class VoiceCommandController(
    private val context: Context,
    private val reports: Reports? = null,
) {
    val store: ModelStore = VoiceModelWorker.store(context)

    val installed: Boolean get() = store.installed

    private fun lang(): CommandLang = if (Locale.getDefault().language == "fr") CommandLang.FR else CommandLang.EN

    private fun battery(): Pair<Double, Double?> {
        val bm = context.getSystemService(BatteryManager::class.java)
        val p = bm?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY) ?: 0
        // No estimate of the autonomy on the phone yet: the level alone.
        return p.toDouble() to null
    }

    /**
     * Listens to one question (RECORD_AUDIO granted by the screen) and answers it; [service]
     * is the location service of the outing, null when none runs.
     */
    fun ask(
        service: TrackingService?,
        source: () -> FrameSource = ::MicSource,
    ) {
        if (!store.installed) return
        thread(name = "voicecmd") {
            val loader = EngineLoader.sherpa(store, RustCommandClassifier.hotwords)
            val vc = VoiceCommands(loader, RustCommandClassifier, { answer(it, service) }, ::lang)
            val out = runCatching { vc.listen(source()) }
            out.onSuccess { o ->
                Log.i(
                    TAG,
                    "voice command: ${o?.heard?.command} « ${o?.heard?.text} » ended ${o?.ended}, load ${o?.loadMs} ms, " +
                        "decode ${o?.decodeMs} ms for ${o?.audioSeconds} s",
                )
            }
            out.onFailure { Log.w(TAG, "voice command", it) }
        }
    }

    private fun answer(
        heard: Heard,
        service: TrackingService?,
    ) {
        val planDir = (Tracking.state.value as? TrackingState.Running)?.planDir
        if (service != null && planDir != null) {
            service.inject { core ->
                val rc = core as RustTrackingCore
                val voice = VoiceAnswers(File(rc.session.planDir()).let { voiceDir(it) })
                val a =
                    voice.use {
                        CommandAnswers(it, ::battery, reports).answer(heard, Outing(rc, planDir, service.stats.said.lastOrNull()), now())
                    }
                val b = a.batch
                Batch(
                    utterances = a.said + (b?.utterances ?: emptyList()),
                    alerts = b?.alerts ?: emptyList(),
                    switches = b?.switches ?: emptyList(),
                    proposals = b?.proposals ?: emptyList(),
                    session = b?.session ?: emptyList(),
                    gps = b?.gps,
                    recap = b?.recap,
                    guidance = b?.guidance ?: core.guidance(),
                )
            }
        } else {
            val a = VoiceAnswers(null).use { CommandAnswers(it, ::battery, reports).answer(heard, null, now()) }
            val announcer = Announcer(context)
            a.said.forEach(announcer::say)
            announcer.afterQueue { announcer.close() }
        }
    }

    private fun now() = System.currentTimeMillis()

    companion object {
        private const val TAG = "skimap"

        /** The voice layer of the pack of the plan (`pack.txt` next to the plan, as the core reads it). */
        fun voiceDir(planDir: File): String? =
            runCatching { File(planDir, "pack.txt").readText().trim() }
                .getOrNull()
                ?.let { File(it, "voice").path }
    }
}
