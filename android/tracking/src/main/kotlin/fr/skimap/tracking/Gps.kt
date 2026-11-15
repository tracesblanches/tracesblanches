package fr.skimap.tracking

import android.location.Location
import androidx.core.location.LocationRequestCompat
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The core's GPS request as a platform request (report 09, section 1): high accuracy,
 * `minUpdateInterval` equal to the interval (fixes asked by other apps do not wake us),
 * batches up to the core's delay (`setMaxUpdateDelayMillis`: the CPU sleeps while the
 * chip keeps the fixes). GNSS off (pause, refuge): a passive request, at most one fix a
 * minute, and the significant motion sensor wakes the outing.
 */
object GpsPolicy {
    /** Shortest fix interval asked, milliseconds (the core's 1 Hz). */
    const val MIN_INTERVAL_MS = 1_000L

    /** Passive fixes during a pause: at most one a minute. */
    const val PASSIVE_MIN_INTERVAL_MS = 60_000L

    /** Batching is only asked when it holds at least two fixes (Android ignores less). */
    fun batchDelayMs(r: GpsRequest): Long = if (r.maxDelayMs >= 2 * r.intervalMs) r.maxDelayMs else 0L

    fun locationRequest(r: GpsRequest): LocationRequestCompat =
        if (!r.gnss) {
            LocationRequestCompat
                .Builder(LocationRequestCompat.PASSIVE_INTERVAL)
                .setMinUpdateIntervalMillis(PASSIVE_MIN_INTERVAL_MS)
                .setQuality(LocationRequestCompat.QUALITY_LOW_POWER)
                .build()
        } else {
            val interval = r.intervalMs.coerceAtLeast(MIN_INTERVAL_MS)
            LocationRequestCompat
                .Builder(interval)
                .setQuality(LocationRequestCompat.QUALITY_HIGH_ACCURACY)
                .setMinUpdateIntervalMillis(interval)
                .setMaxUpdateDelayMillis(batchDelayMs(r.copy(intervalMs = interval)))
                .build()
        }

    /** Whether going from [old] to [new] needs a new platform request (each one may restart an acquisition: no yoyo). */
    fun changes(
        old: GpsRequest?,
        new: GpsRequest,
    ): Boolean =
        old == null ||
            old.gnss != new.gnss ||
            old.intervalMs != new.intervalMs ||
            batchDelayMs(old) != batchDelayMs(new)

    /** The request before the core decided anything: 1 s, to date the start (gps.rs, idle). */
    val START = GpsRequest(GpsMode.IDLE, gnss = true, intervalMs = 1_000L, maxDelayMs = 0L, significantMotion = false)
}

/**
 * Packs a batch of fixes for the core: 24 bytes each (epoch ms, longitude, latitude,
 * little endian). One array per batch, no object per fix.
 */
object FixPacker {
    const val FIX_BYTES = 24

    fun pack(fixes: List<Location>): ByteArray {
        val out = ByteArray(fixes.size * FIX_BYTES)
        val b = ByteBuffer.wrap(out).order(ByteOrder.LITTLE_ENDIAN)
        for (i in fixes.indices) {
            val l = fixes[i]
            b.putLong(l.time)
            b.putDouble(l.longitude)
            b.putDouble(l.latitude)
        }
        return out
    }

    /** Same, from raw values (tests, replay). */
    fun pack(
        times: LongArray,
        lons: DoubleArray,
        lats: DoubleArray,
    ): ByteArray {
        val out = ByteArray(times.size * FIX_BYTES)
        val b = ByteBuffer.wrap(out).order(ByteOrder.LITTLE_ENDIAN)
        for (i in times.indices) {
            b.putLong(times[i])
            b.putDouble(lons[i])
            b.putDouble(lats[i])
        }
        return out
    }
}
