package fr.skimap.sport

import androidx.health.connect.client.records.Record
import java.io.File

/** An outing as the core exports it, with its two files in [dir]. */
object SportFixtures {
    /** 2026-01-15 09:30 UTC. */
    const val START_MS = 1_768_469_400_000L
    private const val STEP_MS = 5_000L
    private const val NEAR_MS = 15_000L
    private const val LAT = 44.8
    private const val LON = 6.7
    private const val STEP_DEG = 1e-4
    private const val STEP_M = 11.1
    private const val ELE_M = 1850.0
    private const val CLIMB_M = 119.0
    private val FIT_HEADER = byteArrayOf(14, 0x20)

    fun outing(
        dir: File,
        heart: List<HeartSample> = emptyList(),
        n: Int = 120,
    ): SharedOuting {
        dir.mkdirs()
        val points =
            (0 until n).map { i ->
                val t = START_MS + i * STEP_MS
                OutingPoint(
                    timeMs = t,
                    lat = LAT + i * STEP_DEG,
                    lon = LON,
                    ele = ELE_M + i,
                    distanceM = i * STEP_M,
                    heartBpm = heart.firstOrNull { kotlin.math.abs(it.timeMs - t) <= NEAR_MS }?.bpm,
                )
            }
        val gpx = File(dir, "skimap-2026-01-15-1030-boucle.gpx").apply { writeText("<gpx/>") }
        val fit = File(dir, "skimap-2026-01-15-1030-boucle.fit").apply { writeBytes(FIT_HEADER) }
        return SharedOuting(
            id = "2026-01-15-boucle",
            title = "Boucle du Chaumoz",
            sport = SportType.NORDIC_SKI,
            startMs = points.first().timeMs,
            endMs = points.last().timeMs,
            distanceM = points.last().distanceM,
            ascentM = CLIMB_M,
            points = points,
            gpx = gpx,
            fit = fit,
            notes = "6,2 km en 1 h 10.",
        )
    }
}

/** Health Connect without Health Connect: what was asked, granted, written and read. */
class FakeHealth(
    var present: Boolean = true,
    var have: Set<String> = emptySet(),
    val heartSamples: List<HeartSample> = emptyList(),
) : HealthApi {
    val written = ArrayList<Record>()
    val heartAsked = ArrayList<Pair<Long, Long>>()

    override fun available(): Boolean = present

    override suspend fun granted(): Set<String> = have

    override suspend fun insert(records: List<Record>): List<String> {
        written += records
        return records.indices.map { "hc-$it" }
    }

    override suspend fun heart(
        startMs: Long,
        endMs: Long,
    ): List<HeartSample> {
        heartAsked += startMs to endMs
        return heartSamples.filter { it.timeMs in startMs..endMs }
    }
}
