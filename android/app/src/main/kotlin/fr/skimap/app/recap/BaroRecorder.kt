package fr.skimap.app.recap

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Handler
import android.os.HandlerThread
import android.os.Process
import android.util.Log
import java.io.File

/**
 * Altitude by the barometer (K8): the pressure of the outing, one sample per second
 * delivered in batches ([BATCH_US], the processor sleeps in between: report 09, §3), written
 * next to the outing (`pressure.txt`, `epoch_ms hPa`) every [FLUSH_LINES] samples. The core
 * fuses it with the relief under the GPS positions for the altitude and the climb of the
 * trace (`crates/ffi/src/recap.rs`). Nothing when the phone has no barometer.
 */
class BaroRecorder private constructor(
    private val sensors: SensorManager,
    private val file: File,
) : SensorEventListener {
    private val thread = HandlerThread("skimap-baro", Process.THREAD_PRIORITY_BACKGROUND).also { it.start() }
    private val handler = Handler(thread.looper)
    private val buffer = StringBuilder()
    private var lines = 0

    // Event timestamps are nanoseconds of the boot clock: turned into wall-clock time.
    private val offsetMs = System.currentTimeMillis() - android.os.SystemClock.elapsedRealtime()

    override fun onSensorChanged(e: SensorEvent) {
        val ms = e.timestamp / NANOS_PER_MS + offsetMs
        buffer
            .append(ms)
            .append(' ')
            .append(e.values[0])
            .append('\n')
        if (++lines >= FLUSH_LINES) flush()
    }

    override fun onAccuracyChanged(
        sensor: Sensor,
        accuracy: Int,
    ) = Unit

    private fun flush() {
        if (buffer.isEmpty()) return
        runCatching { file.appendText(buffer.toString()) }.onFailure { Log.w("skimap", "baro", it) }
        buffer.setLength(0)
        lines = 0
    }

    fun stop() {
        sensors.unregisterListener(this)
        handler.post {
            flush()
            thread.quitSafely()
        }
    }

    companion object {
        const val FILE = "pressure.txt"
        private const val PERIOD_US = 1_000_000
        private const val BATCH_US = 60_000_000
        private const val FLUSH_LINES = 120
        private const val NANOS_PER_MS = 1_000_000L

        /** Starts recording into the outing [dir]; `null` without a barometer. */
        @Suppress("ReturnCount")
        fun start(
            context: Context,
            dir: String,
        ): BaroRecorder? {
            val sm = context.getSystemService(SensorManager::class.java) ?: return null
            val s = sm.getDefaultSensor(Sensor.TYPE_PRESSURE) ?: return null
            val r = BaroRecorder(sm, File(dir, FILE))
            sm.registerListener(r, s, PERIOD_US, BATCH_US, r.handler)
            return r
        }
    }
}
