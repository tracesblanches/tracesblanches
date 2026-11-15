package fr.skimap.sport

import android.content.Context
import android.os.Build
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.permission.HealthPermission
import androidx.health.connect.client.records.ExerciseRoute
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.health.connect.client.records.metadata.Device
import androidx.health.connect.client.records.metadata.Metadata
import androidx.health.connect.client.units.Length
import java.io.File
import java.time.Instant
import java.time.ZoneId

/** The session written to Health Connect: skiing, with the recorded route. */
object HealthRecords {
    /** Health Connect has one skiing type for every kind of skiing. */
    const val TYPE = ExerciseSessionRecord.EXERCISE_TYPE_SKIING

    /** Version of what is written for one outing: written again, the session is replaced. */
    private const val VERSION = 1L

    private const val AFTER_LAST_MS = 1_000L

    fun session(
        outing: Outing,
        track: Track,
        zone: ZoneId,
        device: Device,
    ): ExerciseSessionRecord {
        require(track.points.size >= 2) { "trace without two points" }
        val start = Instant.ofEpochMilli(track.startMs)
        // Health Connect wants every route point before the end: the end is one second after the last.
        val end = Instant.ofEpochMilli(track.endMs + AFTER_LAST_MS)
        val route =
            ExerciseRoute(
                track.points.map {
                    ExerciseRoute.Location(
                        time = Instant.ofEpochMilli(it.timeMs),
                        latitude = it.lat,
                        longitude = it.lon,
                        altitude = it.ele?.let(Length::meters),
                    )
                },
            )
        return ExerciseSessionRecord(
            startTime = start,
            startZoneOffset = zone.rules.getOffset(start),
            endTime = end,
            endZoneOffset = zone.rules.getOffset(end),
            metadata = Metadata.activelyRecorded(device, clientRecordId = "skimap-${outing.id}", clientRecordVersion = VERSION),
            exerciseType = TYPE,
            title = outing.name,
            notes = outing.description,
            exerciseRoute = route,
        )
    }
}

/** Health Connect on this phone: available, the permissions, the write. */
class HealthConnectSender(
    private val context: Context,
) : HealthWriter {
    /** Platform module (Android 14+) or the app (older), and up to date. */
    fun status(): Int = HealthConnectClient.getSdkStatus(context)

    override fun available(): Boolean = status() == HealthConnectClient.SDK_AVAILABLE

    override suspend fun granted(): Boolean {
        if (!available()) return false
        val have = HealthConnectClient.getOrCreate(context).permissionController.getGrantedPermissions()
        return have.containsAll(PERMISSIONS)
    }

    /** Writes the outing's session with its route; its id in Health Connect. */
    override suspend fun insert(outing: Outing): String {
        val track = Gpx.read(File(outing.gpx).readText())
        val device = Device(type = Device.TYPE_PHONE, manufacturer = Build.MANUFACTURER, model = Build.MODEL)
        val record = HealthRecords.session(outing, track, ZoneId.systemDefault(), device)
        val ids = HealthConnectClient.getOrCreate(context).insertRecords(listOf(record)).recordIdsList
        return ids.first()
    }

    companion object {
        /** Write only: the session and its route. Nothing is ever read. */
        val PERMISSIONS =
            setOf(
                HealthPermission.getWritePermission(ExerciseSessionRecord::class),
                HealthPermission.PERMISSION_WRITE_EXERCISE_ROUTE,
            )
    }
}
