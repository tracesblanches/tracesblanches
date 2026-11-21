package fr.skimap.sport

import android.content.Context
import android.os.Build
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.permission.HealthPermission
import androidx.health.connect.client.records.DistanceRecord
import androidx.health.connect.client.records.ElevationGainedRecord
import androidx.health.connect.client.records.ExerciseRoute
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.health.connect.client.records.HeartRateRecord
import androidx.health.connect.client.records.Record
import androidx.health.connect.client.records.metadata.Device
import androidx.health.connect.client.records.metadata.Metadata
import androidx.health.connect.client.request.ReadRecordsRequest
import androidx.health.connect.client.time.TimeRangeFilter
import androidx.health.connect.client.units.Length
import java.time.Instant
import java.time.ZoneId

/**
 * Health Connect as the share needs it (a fake in the tests): there, the permissions
 * granted, the write, the heart rate of the hours skied.
 */
interface HealthApi {
    /** Health Connect is on the phone and up to date. */
    fun available(): Boolean

    suspend fun granted(): Set<String>

    /** Writes [records]; their ids in Health Connect. */
    suspend fun insert(records: List<Record>): List<String>

    /** The heart rate samples between [startMs] and [endMs] (a watch's, through Health Connect). */
    suspend fun heart(
        startMs: Long,
        endMs: Long,
    ): List<HeartSample>
}

/** What is asked of Health Connect, at the share only, never at the start of the app. */
object HealthPermissions {
    val SESSION = HealthPermission.getWritePermission(ExerciseSessionRecord::class)
    const val ROUTE = HealthPermission.PERMISSION_WRITE_EXERCISE_ROUTE
    val DISTANCE = HealthPermission.getWritePermission(DistanceRecord::class)
    val ELEVATION = HealthPermission.getWritePermission(ElevationGainedRecord::class)

    /** Read, only to put the heart rate of the outing into the shared file. */
    val HEART = HealthPermission.getReadPermission(HeartRateRecord::class)

    /** Everything asked at once; the user may grant part of it. */
    val ALL = setOf(SESSION, ROUTE, DISTANCE, ELEVATION, HEART)
}

/**
 * The records of an outing in Health Connect: the exercise session (skiing: Health
 * Connect has one type for every kind of skiing) with its route, the distance and the
 * climb, each as allowed by [granted]. Stable client ids: shared again, the outing
 * replaces its records instead of adding a second session.
 */
object HealthRecords {
    const val TYPE = ExerciseSessionRecord.EXERCISE_TYPE_SKIING

    /** Version of what is written for one outing. */
    private const val VERSION = 1L

    /** Health Connect wants every route point before the end: the end is one second after the last. */
    private const val AFTER_LAST_MS = 1_000L

    fun of(
        o: SharedOuting,
        granted: Set<String>,
        zone: ZoneId,
        device: Device,
    ): List<Record> {
        if (HealthPermissions.SESSION !in granted || o.points.size < 2) return emptyList()
        val start = Instant.ofEpochMilli(o.startMs)
        val end = Instant.ofEpochMilli(o.endMs + AFTER_LAST_MS)
        val startOffset = zone.rules.getOffset(start)
        val endOffset = zone.rules.getOffset(end)

        fun meta(part: String) = Metadata.activelyRecorded(device, clientRecordId = "skimap-${o.id}-$part", clientRecordVersion = VERSION)
        val route = if (HealthPermissions.ROUTE in granted) route(o) else null
        return buildList {
            add(
                ExerciseSessionRecord(
                    startTime = start,
                    startZoneOffset = startOffset,
                    endTime = end,
                    endZoneOffset = endOffset,
                    metadata = meta("session"),
                    exerciseType = TYPE,
                    title = o.title,
                    notes = o.notes,
                    exerciseRoute = route,
                ),
            )
            if (HealthPermissions.DISTANCE in granted) {
                add(
                    DistanceRecord(
                        startTime = start,
                        startZoneOffset = startOffset,
                        endTime = end,
                        endZoneOffset = endOffset,
                        distance = Length.meters(o.distanceM),
                        metadata = meta("distance"),
                    ),
                )
            }
            val climb = o.ascentM
            if (HealthPermissions.ELEVATION in granted && climb != null) {
                add(
                    ElevationGainedRecord(
                        startTime = start,
                        startZoneOffset = startOffset,
                        endTime = end,
                        endZoneOffset = endOffset,
                        elevation = Length.meters(climb),
                        metadata = meta("elevation"),
                    ),
                )
            }
        }
    }

    /** Every point of the trace, with its altitude when known. */
    private fun route(o: SharedOuting) =
        ExerciseRoute(
            o.points.map {
                ExerciseRoute.Location(
                    time = Instant.ofEpochMilli(it.timeMs),
                    latitude = it.lat,
                    longitude = it.lon,
                    altitude = it.ele?.let(Length::meters),
                )
            },
        )

    /** The phone that recorded the outing. */
    fun phone(): Device = Device(type = Device.TYPE_PHONE, manufacturer = Build.MANUFACTURER, model = Build.MODEL)
}

/** Health Connect on this phone (platform module on Android 14+, the app before). */
class HealthConnectApi(
    private val context: Context,
) : HealthApi {
    override fun available(): Boolean = HealthConnectClient.getSdkStatus(context) == HealthConnectClient.SDK_AVAILABLE

    private fun client() = HealthConnectClient.getOrCreate(context)

    override suspend fun granted(): Set<String> = if (available()) client().permissionController.getGrantedPermissions() else emptySet()

    override suspend fun insert(records: List<Record>): List<String> = client().insertRecords(records).recordIdsList

    override suspend fun heart(
        startMs: Long,
        endMs: Long,
    ): List<HeartSample> {
        val out = ArrayList<HeartSample>()
        var token: String? = null
        do {
            val r =
                client().readRecords(
                    ReadRecordsRequest(
                        HeartRateRecord::class,
                        timeRangeFilter = TimeRangeFilter.between(Instant.ofEpochMilli(startMs), Instant.ofEpochMilli(endMs)),
                        pageToken = token,
                    ),
                )
            r.records.flatMap { it.samples }.mapTo(out) { HeartSample(it.time.toEpochMilli(), it.beatsPerMinute.toInt()) }
            token = r.pageToken
        } while (token != null)
        return out
    }
}
