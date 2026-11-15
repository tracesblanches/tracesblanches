package fr.skimap.app.recap

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener2
import android.hardware.SensorManager
import android.os.Handler
import android.os.HandlerThread
import android.os.Process
import android.util.Log
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Altitude by the barometer (K8): the pressure of the outing, one sample per second
 * delivered in batches ([BATCH_US], the processor sleeps in between: report 09, §3), written
 * next to the outing (`pressure.txt`, `epoch_ms hPa`) every [FLUSH_LINES] samples. The core
 * fuses it with the relief under the GPS positions for the altitude and the climb of the
 * trace (`crates/ffi/src/recap.rs`). Nothing when the phone has no barometer. At the end the
 * samples still batched in the sensor hub are asked for and written before [stop] returns,
 * so that the recap made right after sees the whole outing.
 */
class BaroRecorder private constructor(
    private val sensors: SensorManager,
    private val file: File,
) : SensorEventListener2 {
    private val thread = HandlerThread("skimap-baro", Process.THREAD_PRIORITY_BACKGROUND).also { it.start() }
    private val handler = Handler(thread.looper)
    private val buffer = StringBuilder()
    private var lines = 0
    private val drained = CountDownLatch(1)

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

    /** The hub has delivered every sample batched before [stop] asked for them. */
    override fun onFlushCompleted(sensor: Sensor) {
        drained.countDown()
    }

    private fun flush() {
        if (buffer.isEmpty()) return
        runCatching { file.appendText(buffer.toString()) }.onFailure { Log.w("skimap", "baro", it) }
        buffer.setLength(0)
        lines = 0
    }

    /**
     * Ends the recording: the batched samples are delivered (sensor flush) and the file
     * written before it returns, each wait bounded ([WAIT_MS]). Not on the recorder's thread.
     */
    fun stop() {
        if (sensors.flush(this)) drained.await(WAIT_MS, TimeUnit.MILLISECONDS)
        sensors.unregisterListener(this)
        val written = CountDownLatch(1)
        handler.post {
            flush()
            thread.quitSafely()
            written.countDown()
        }
        written.await(WAIT_MS, TimeUnit.MILLISECONDS)
    }

    companion object {
        const val FILE = "pressure.txt"
        private const val PERIOD_US = 1_000_000
        private const val BATCH_US = 60_000_000
        private const val FLUSH_LINES = 120
        private const val NANOS_PER_MS = 1_000_000L
        private const val WAIT_MS = 1_000L

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
