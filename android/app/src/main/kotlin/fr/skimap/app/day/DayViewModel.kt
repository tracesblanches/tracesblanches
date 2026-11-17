package fr.skimap.app.day

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.skimap.app.appLang
import fr.skimap.app.durationText
import fr.skimap.core.DayPlanView
import fr.skimap.core.DayPlanner
import fr.skimap.core.DayRequest
import fr.skimap.core.DayStart
import fr.skimap.core.GroupSpec
import fr.skimap.core.GroupView
import fr.skimap.core.Lang
import fr.skimap.core.MorningView
import fr.skimap.core.TrackingException
import fr.skimap.core.daySites
import fr.skimap.core.dayStarts
import fr.skimap.core.groupNew
import fr.skimap.core.listPacks
import fr.skimap.core.openDayPlanner
import fr.skimap.core.readDayPlan
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.time.LocalDate
import java.time.LocalTime

/** What the skier asks in the morning (J5): a few choices, all with a default. */
data class DayInputs(
    val date: LocalDate,
    val start: DayStart? = null,
    val budgetMin: Int = 120,
    val skating: Boolean = false,
    /** « 16:00 », or none. */
    val returnBefore: String? = null,
    /** Green and blue pistes only. */
    val easyOnly: Boolean = false,
)

/** The morning screen. */
sealed interface DayUi {
    data object Loading : DayUi

    /** No pack of a site the phone can plan on. */
    data object Unavailable : DayUi

    data class Ready(
        val siteId: String,
        val siteName: String,
        val starts: List<DayStart>,
        val inputs: DayInputs,
        val morning: MorningView?,
        val computing: Boolean,
        val choice: Int,
        val groups: List<GroupView>,
        val group: GroupView?,
        val contact: Contact?,
        val error: String? = null,
    ) : DayUi
}

private const val MINUTES_PER_HOUR = 60
private const val DAY_START = "08:00"
private const val QUARTER_MIN = 15
private const val LEAD_MIN = 10
private const val DAY_START_MIN = 8 * 60
private const val LAST_START_MIN = 23 * 60 + 45

/**
 * The earliest start of a day: 8 h, or today the next quarter hour at least 10 min from
 * now (a plan validated at 13 h 25 does not start at 11 h).
 */
fun earliestStart(
    date: LocalDate,
    today: LocalDate,
    now: LocalTime,
): String {
    if (date != today) return DAY_START
    // Minutes of the day, 10 min from now, up to the next quarter hour; 8 h at the earliest.
    val soon = now.hour * MINUTES_PER_HOUR + now.minute + LEAD_MIN
    val quarter = (soon + QUARTER_MIN - 1) / QUARTER_MIN * QUARTER_MIN
    val t = quarter.coerceIn(DAY_START_MIN, LAST_START_MIN)
    return "%02d:%02d".format(t / MINUTES_PER_HOUR, t % MINUTES_PER_HOUR)
}

/**
 * The core's request for the choices of the morning (`null` without a start). [earliest]:
 * the earliest start the best time may choose, « 08:00 », or later today.
 */
fun dayRequest(
    i: DayInputs,
    earliest: String = DAY_START,
    lang: Lang = appLang(),
): DayRequest? {
    val s = i.start ?: return null
    val label =
        buildString {
            val w = RequestWords.of(lang)
            append(durationText(i.budgetMin, lang))
            append(if (i.skating) ", skating" else ", ${w.classic}")
            if (i.easyOnly) append(", ${w.easy}")
            i.returnBefore?.let { append(", ${w.backBefore} ${it.replace(":", w.clockSeparator)}") }
        }
    return DayRequest(
        label = label,
        start = s,
        date = i.date.toString(),
        startTime = null,
        earliest = earliest,
        budgetMin = i.budgetMin.toDouble(),
        skating = i.skating,
        returnBefore = i.returnBefore,
        maxDifficulty = if (i.easyOnly) "easy" else null,
    )
}

/** The core's morning, behind an interface (tests use a fake). */
interface DayBackend {
    /** Packs a day can be planned on: (directory, site id, site name). */
    fun packs(root: File): List<Triple<String, String, String>>

    fun open(
        packDir: String,
        workDir: String,
    ): Planner

    fun starts(siteId: String): List<DayStart>

    fun readPlan(planDir: String): DayPlanView?

    interface Planner {
        fun morning(
            request: DayRequest,
            groupJson: String?,
        ): MorningView

        fun validate(
            choice: Int,
            groupJson: String?,
            planDir: String,
        ): DayPlanView
    }
}

/** [DayBackend] on the Rust core. */
object RustDayBackend : DayBackend {
    override fun packs(root: File): List<Triple<String, String, String>> {
        val sites = daySites().toSet()
        return listPacks(root.absolutePath).filter { it.error == null && it.siteId in sites }.map { Triple(it.dir, it.siteId, it.siteName) }
    }

    override fun open(
        packDir: String,
        workDir: String,
    ): DayBackend.Planner {
        val p: DayPlanner = openDayPlanner(packDir, workDir, appLang())
        return object : DayBackend.Planner {
            override fun morning(
                request: DayRequest,
                groupJson: String?,
            ) = p.morning(request, groupJson)

            override fun validate(
                choice: Int,
                groupJson: String?,
                planDir: String,
            ) = p.validate(choice.toUInt(), groupJson, planDir)
        }
    }

    override fun starts(siteId: String): List<DayStart> = dayStarts(siteId, appLang())

    override fun readPlan(planDir: String): DayPlanView? = runCatching { readDayPlan(planDir, appLang()) }.getOrNull()
}

/** Words of the label of a request (the core keeps it with the plan), in the app's language. */
private data class RequestWords(
    val classic: String,
    val easy: String,
    val backBefore: String,
    val clockSeparator: String,
    /** Error of « Valider la journée » without a loop. */
    val noLoop: String,
) {
    companion object {
        private val FR = RequestWords("classique", "vertes et bleues", "retour avant", " h ", "aucune boucle à valider")
        private val EN = RequestWords("classic", "green and blue", "back before", ":", "no loop to validate")

        fun of(lang: Lang): RequestWords = if (lang == Lang.FR) FR else EN
    }
}

/**
 * The morning (J5, K9, C11): opens the pack's layers once, recomputes the loops when a
 * choice changes, validates the day in one gesture. Every core call runs on [io].
 */
class DayViewModel(
    private val backend: DayBackend,
    private val store: DayStore,
    private val packsDir: () -> File,
    private val workDir: () -> File,
    private val preferredSite: String?,
    private val today: LocalDate,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val now: () -> LocalTime = { LocalTime.now() },
    /** An error of the core in the language of the app ([fr.skimap.app.ui.CoreErrors]). */
    private val errorText: (Throwable) -> String = { it.message.orEmpty() },
) : ViewModel() {
    private val mutable = MutableStateFlow<DayUi>(DayUi.Loading)
    val state: StateFlow<DayUi> = mutable.asStateFlow()
    private val plan = MutableStateFlow<DayPlanView?>(null)

    /** The validated plan (or the one being followed). */
    val validated: StateFlow<DayPlanView?> = plan.asStateFlow()
    private var planner: DayBackend.Planner? = null
    private var computing: Job? = null
    private var inputs = DayInputs(today)

    init {
        viewModelScope.launch { load() }
    }

    private suspend fun load() {
        val loaded =
            withContext(io) {
                val packs = backend.packs(packsDir())
                val pick = packs.firstOrNull { it.second == preferredSite } ?: packs.firstOrNull()
                if (pick == null) {
                    null
                } else {
                    val t0 = System.nanoTime()
                    val p = backend.open(pick.first, workDir().absolutePath)
                    android.util.Log.i(
                        "skimap",
                        "day: ${pick.second} layers opened in ${java.util.concurrent.TimeUnit.NANOSECONDS.toMillis(
                            System.nanoTime() - t0,
                        )} ms",
                    )
                    Triple(pick, p, backend.starts(pick.second))
                }
            }
        if (loaded == null) {
            mutable.value = DayUi.Unavailable
            return
        }
        val (pick, p, starts) = loaded
        planner = p
        inputs = inputs.copy(start = starts.firstOrNull())
        val groups = withContext(io) { store.groups() }
        val selected = store.selectedGroup()
        mutable.value =
            DayUi.Ready(
                siteId = pick.second,
                siteName = pick.third,
                starts = starts,
                inputs = inputs,
                morning = null,
                computing = true,
                choice = 0,
                groups = groups,
                group = groups.firstOrNull { it.spec.name == selected } ?: groups.firstOrNull(),
                contact = store.contact(),
            )
        recompute()
    }

    /** Opens the plan already written in [planDir] (an outing under way). */
    fun showPlan(planDir: String) {
        viewModelScope.launch { plan.value = withContext(io) { backend.readPlan(planDir) } }
    }

    fun setInputs(f: (DayInputs) -> DayInputs) {
        inputs = f(inputs)
        update { it.copy(inputs = inputs) }
        recompute()
    }

    fun choose(i: Int) = update { it.copy(choice = i) }

    fun selectGroup(g: GroupView?) {
        store.selectGroup(g?.spec?.name)
        update { it.copy(group = g) }
        recompute()
    }

    fun addGroup(spec: GroupSpec) {
        viewModelScope.launch {
            val g = withContext(io) { groupNew(spec, appLang()).also { store.putGroup(it) } }
            update { it.copy(groups = store.groups()) }
            selectGroup(g)
        }
    }

    fun setContact(c: Contact?) {
        store.setContact(c)
        update { it.copy(contact = c) }
    }

    private fun update(f: (DayUi.Ready) -> DayUi.Ready) {
        mutable.update { if (it is DayUi.Ready) f(it) else it }
    }

    private fun recompute() {
        val p = planner
        val s = state.value as? DayUi.Ready
        val req = dayRequest(inputs, earliestStart(inputs.date, today, now()))
        if (p == null || s == null || req == null) return
        computing?.cancel()
        update { it.copy(computing = true, error = null) }
        computing =
            viewModelScope.launch {
                val r = withContext(io) { runCatching { p.morning(req, s.group?.modelJson) } }
                r
                    .onSuccess { m ->
                        android.util.Log.i("skimap", "day: morning ${req.label} in ${m.computeMs} ms, ${m.loops.size} loops")
                        update { it.copy(morning = m, computing = false, choice = 0) }
                    }.onFailure { e -> update { it.copy(computing = false, error = errorText(e)) } }
            }
    }

    /**
     * « Valider la journée »: the plan of the chosen loop, written to a new directory.
     * The caller starts the location service with it.
     */
    suspend fun validate(): Result<DayPlanView> {
        val p = planner
        val s = state.value as? DayUi.Ready
        if (p == null || s == null || s.morning?.loops.isNullOrEmpty()) {
            return Result.failure(IllegalStateException(RequestWords.of(appLang()).noLoop))
        }
        return withContext(io) {
            try {
                val dir = store.newPlanDir(s.inputs.date.toString())
                val v = p.validate(s.choice, s.group?.modelJson, dir.absolutePath)
                store.setPlanGroup(dir.absolutePath, s.group?.spec?.name)
                plan.value = v
                Result.success(v)
            } catch (e: TrackingException) {
                Result.failure(e)
            }
        }
    }
}
