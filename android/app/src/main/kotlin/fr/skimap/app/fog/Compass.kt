package fr.skimap.app.fog

import android.hardware.GeomagneticField
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Calibration of the magnetometer, as the platform reports it. */
enum class Calibration { OK, LOW, NONE }

/** The compass seen by the screen: true heading (degrees) and calibration. */
data class CompassReading(
    val headingDeg: Double?,
    val calibration: Calibration,
)

/**
 * The phone's compass (K7), only while the bearing screen is shown: the rotation vector
 * (magnetometer, accelerometer and gyroscope fused by the platform) gives the azimuth, the
 * magnetic declination of the place turns it to true north, and the accuracy of the
 * magnetometer says whether a figure of eight is needed. Screen off, nothing is read: the
 * voice guidance uses the direction of travel (GPS), never the compass in a pocket.
 */
class Compass(
    private val sensors: SensorManager,
) : SensorEventListener {
    private val mutable = MutableStateFlow(CompassReading(null, Calibration.NONE))
    val reading: StateFlow<CompassReading> = mutable.asStateFlow()
    private val rotation = FloatArray(9)
    private val orientation = FloatArray(3)
    private var declination = 0.0
    private var accuracy = SensorManager.SENSOR_STATUS_UNRELIABLE

    /** Starts listening; `false` without a rotation vector sensor (no compass). */
    fun start(): Boolean {
        val rv = sensors.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR) ?: return false
        sensors.registerListener(this, rv, SensorManager.SENSOR_DELAY_UI)
        sensors.getDefaultSensor(Sensor.TYPE_MAGNETIC_FIELD)?.let {
            sensors.registerListener(this, it, SensorManager.SENSOR_DELAY_NORMAL)
        }
        mutable.value = CompassReading(null, calibration(accuracy))
        return true
    }

    fun stop() = sensors.unregisterListener(this)

    /** The place (declination of the magnetic north). */
    fun setPlace(
        lat: Double,
        lon: Double,
        altitudeM: Double,
        timeMs: Long,
    ) {
        declination = GeomagneticField(lat.toFloat(), lon.toFloat(), altitudeM.toFloat(), timeMs).declination.toDouble()
    }

    override fun onSensorChanged(e: SensorEvent) {
        when (e.sensor.type) {
            Sensor.TYPE_ROTATION_VECTOR -> {
                SensorManager.getRotationMatrixFromVector(rotation, e.values)
                SensorManager.getOrientation(rotation, orientation)
                mutable.value = CompassReading(trueHeading(orientation[0].toDouble(), declination), calibration(accuracy))
            }

            Sensor.TYPE_MAGNETIC_FIELD -> {
                if (e.accuracy != accuracy) onAccuracyChanged(e.sensor, e.accuracy)
            }
        }
    }

    override fun onAccuracyChanged(
        sensor: Sensor,
        accuracy: Int,
    ) {
        if (sensor.type == Sensor.TYPE_MAGNETIC_FIELD) {
            this.accuracy = accuracy
            mutable.value = mutable.value.copy(calibration = calibration(accuracy))
        }
    }

    companion object {
        /** True heading, degrees 0–360, of an azimuth (radians, magnetic) and a declination (degrees, east positive). */
        fun trueHeading(
            azimuthRad: Double,
            declinationDeg: Double,
        ): Double = ((Math.toDegrees(azimuthRad) + declinationDeg) % FULL_TURN + FULL_TURN) % FULL_TURN

        /** Calibration of a magnetometer accuracy: high or medium is calibrated. */
        fun calibration(accuracy: Int): Calibration =
            when (accuracy) {
                SensorManager.SENSOR_STATUS_ACCURACY_HIGH, SensorManager.SENSOR_STATUS_ACCURACY_MEDIUM -> Calibration.OK
                else -> Calibration.LOW
            }

        private const val FULL_TURN = 360.0
    }
}
