package fr.skimap.tracking

/**
 * Android's step counter (`TYPE_STEP_COUNTER`: steps since boot, an event when it changes)
 * as the core's step reports (X10): the steps of each [periodMs] window, 0 when none came
 * (the core needs the silence too: no stride where a skier strides is a stop). A window is
 * closed only once every event up to its end is known ([close] after a flush of the
 * sensor), so that a report is never sent before the steps it should hold.
 */
class StepReports(
    private val periodMs: Long = PERIOD_MS,
) {
    private var last: Long? = null
    private var counted = 0L
    private var from: Long? = null
    private val ready = ArrayList<StepReport>()

    /** An event of the counter: [total] steps since boot at [tMs] (epoch milliseconds). */
    fun sample(
        tMs: Long,
        total: Long,
    ) {
        val l = last
        if (l == null || from == null) {
            // The first event only sets the origin of the count.
            last = total
            from = tMs
            return
        }
        close(tMs)
        // A counter that went back (the phone restarted its sensor) starts again from here.
        counted += if (total >= l) total - l else 0L
        last = total
    }

    /** Closes the windows that ended by [untilMs]: every event up to then is known. */
    fun close(untilMs: Long) {
        var f = from ?: return
        while (f + periodMs <= untilMs) {
            f += periodMs
            ready += StepReport(f, counted.coerceAtMost(Int.MAX_VALUE.toLong()).toInt())
            counted = 0L
        }
        from = f
    }

    /** The reports closed since the last call, oldest first. */
    fun drain(): List<StepReport> {
        if (ready.isEmpty()) return emptyList()
        val r = ArrayList(ready)
        ready.clear()
        return r
    }

    companion object {
        /** Window of a report (the core's bench reports every 10 s). */
        const val PERIOD_MS = 10_000L
    }
}
