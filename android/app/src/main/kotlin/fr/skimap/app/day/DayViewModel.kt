package fr.skimap.app.day

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.skimap.core.DayPlanView
import fr.skimap.core.DayPlanner
import fr.skimap.core.DayRequest
import fr.skimap.core.DayStart
import fr.skimap.core.GroupSpec
import fr.skimap.core.GroupView
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
    data class Unavailable(
        val message: String,
    ) : DayUi

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

/** The core's request for the choices of the morning (`null` without a start). */
fun dayRequest(i: DayInputs): DayRequest? {
    val s = i.start ?: return null
    val h = i.budgetMin / MINUTES_PER_HOUR
    val m = i.budgetMin % MINUTES_PER_HOUR
    val label =
        buildString {
            append(if (m == 0) "$h h" else "$h h ${"%02d".format(m)}")
            append(if (i.skating) ", skating" else ", classique")
            if (i.easyOnly) append(", vertes et bleues")
            i.returnBefore?.let { append(", retour avant ${it.replace(":", " h ")}") }
        }
    return DayRequest(
        label = label,
        start = s,
        date = i.date.toString(),
        startTime = null,
        earliest = "08:00",
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
        val p: DayPlanner = openDayPlanner(packDir, workDir)
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

    override fun starts(siteId: String): List<DayStart> = dayStarts(siteId)

    override fun readPlan(planDir: String): DayPlanView? = runCatching { readDayPlan(planDir) }.getOrNull()
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
    today: LocalDate,
    private val io: CoroutineDispatcher = Dispatchers.IO,
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
                    val p = backend.open(pick.first, workDir().absolutePath)
                    Triple(pick, p, backend.starts(pick.second))
                }
            }
        if (loaded == null) {
            mutable.value = DayUi.Unavailable("Aucun pack de Cervières ou de Chapelle-des-Bois sur ce téléphone.")
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
            val g = withContext(io) { groupNew(spec).also { store.putGroup(it) } }
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
        val req = dayRequest(inputs)
        if (p == null || s == null || req == null) return
        computing?.cancel()
        update { it.copy(computing = true, error = null) }
        computing =
            viewModelScope.launch {
                val r = withContext(io) { runCatching { p.morning(req, s.group?.modelJson) } }
                r
                    .onSuccess { m -> update { it.copy(morning = m, computing = false, choice = 0) } }
                    .onFailure { e -> update { it.copy(computing = false, error = e.message) } }
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
            return Result.failure(IllegalStateException("aucune boucle à valider"))
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
