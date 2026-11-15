package fr.skimap.community

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/** A position of the phone. */
data class Fix(
    val lon: Double,
    val lat: Double,
    val atMs: Long,
)

/** State of the report screen. */
data class ReportState(
    val position: Fix? = null,
    val locating: Boolean = true,
    /** The last report made here (photo, cancel). */
    val last: QueuedReport? = null,
    val photoAdded: Boolean = false,
    val pending: Int = 0,
    val around: List<ServerReport> = emptyList(),
    /** Null: not loaded yet; false: the server could not be reached. */
    val aroundLoaded: Boolean? = null,
    val mine: Set<String> = emptySet(),
    val voted: Set<String> = emptySet(),
    val area: AreaContext? = null,
)

/** State of « Défis et import Strava »: challenges and Strava import (the outings are the app's history). */
data class OutingsState(
    val challenges: List<Challenge> = emptyList(),
    val loading: Boolean = true,
    val importing: Boolean = false,
    val imported: ImportSummary? = null,
    val importError: String? = null,
)

/** State of the OSM note screen. */
data class OsmState(
    val correction: Correction = Correction.WRONG_NAME,
    val comment: String = "",
    val note: OsmNote? = null,
    val error: String? = null,
    val sending: Boolean = false,
    /** URL of the note once sent. */
    val sent: String? = null,
    /** Host the note goes to, shown before sending. */
    val host: String = "",
)

/** The reports side: the queue, the server, the device, the send job. */
class ReportsBackend(
    val outbox: ReportOutbox,
    val api: CommunityApi,
    val device: () -> String,
    val mine: MyReports,
    val enqueue: () -> Unit,
)

/** Where the screens take their data: the reports side and the core. */
class CommunityDeps(
    val core: CommunityCore,
    val reports: ReportsBackend,
    val routesJson: () -> String,
    val now: () -> Long = System::currentTimeMillis,
)

class CommunityViewModel(
    private val deps: CommunityDeps,
    private val siteId: String?,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableReport = MutableStateFlow(ReportState())
    val report: StateFlow<ReportState> = mutableReport.asStateFlow()
    private val mutableOutings = MutableStateFlow(OutingsState())
    val outings: StateFlow<OutingsState> = mutableOutings.asStateFlow()

    /** Opens the report screen: the area, the queue, the reports around. */
    fun openReports() {
        viewModelScope.launch {
            val area = withContext(io) { deps.core.area(siteId) }
            val pending = withContext(io) { deps.reports.outbox.size() }
            mutableReport.update { it.copy(area = area, pending = pending, mine = deps.reports.mine.all()) }
            refreshAround()
        }
    }

    fun setPosition(fix: Fix?) {
        mutableReport.update { it.copy(position = fix ?: it.position, locating = false) }
    }

    /** The one gesture (V9): queued at once with the time and the position, sent when it can. */
    fun report(kind: ReportKind) {
        val p = report.value.position ?: return
        val now = deps.now()
        viewModelScope.launch {
            val q = withContext(io) { deps.reports.outbox.add(kind, p.lon, p.lat, now) }
            mutableReport.update { it.copy(last = q, photoAdded = false, pending = it.pending + 1) }
            deps.reports.enqueue()
        }
    }

    /** Photo of the last report, already reduced. */
    fun attachPhoto(jpeg: ByteArray) {
        val last = report.value.last ?: return
        viewModelScope.launch {
            val ok = withContext(io) { deps.reports.outbox.attachPhoto(last.localId, jpeg) }
            mutableReport.update { it.copy(photoAdded = ok) }
            deps.reports.enqueue()
        }
    }

    /** Cancels the last report if it is still in the queue, else deletes it on the server. */
    fun cancelLast() {
        val last = report.value.last ?: return
        viewModelScope.launch {
            withContext(io) {
                val serverId = deps.reports.outbox.serverId(last.localId)
                val queued =
                    deps.reports.outbox
                        .pending()
                        .any { it.localId == last.localId }
                deps.reports.outbox.remove(last.localId)
                if (!queued || serverId != null) {
                    // Already sent: the newest of mine at that place.
                    val id =
                        serverId ?: report.value.around
                            .firstOrNull { it.id in deps.reports.mine.all() && it.kind == last.kind }
                            ?.id
                    if (id != null &&
                        runCatching { deps.reports.api.delete(id, deps.reports.device()) }.getOrDefault(false)
                    ) {
                        deps.reports.mine.remove(id)
                    }
                }
            }
            mutableReport.update { it.copy(last = null, pending = deps.reports.outbox.size()) }
            refreshAround()
        }
    }

    fun refreshAround() {
        val area = report.value.area
        val bbox = area?.bbox ?: report.value.position?.let { aroundBox(it) } ?: return
        viewModelScope.launch {
            val r = withContext(io) { runCatching { deps.reports.api.list(bbox) } }
            mutableReport.update {
                it.copy(
                    around = r.getOrDefault(it.around),
                    aroundLoaded = r.isSuccess,
                    mine = deps.reports.mine.all(),
                    pending = deps.reports.outbox.size(),
                )
            }
        }
    }

    fun vote(
        id: String,
        confirm: Boolean,
    ) {
        viewModelScope.launch {
            val r = withContext(io) { runCatching { deps.reports.api.vote(id, deps.reports.device(), confirm) }.getOrNull() }
            mutableReport.update { s ->
                s.copy(
                    voted = s.voted + id,
                    around = s.around.map { if (it.id == id && r != null) r else it },
                )
            }
        }
    }

    /** « Défis et import Strava »: the challenges, computed on the phone from the app's history of the outings. */
    fun openOutings() {
        viewModelScope.launch {
            val ch =
                withContext(io) {
                    val roots = listOf(deps.core.outingsRoot())
                    val area = deps.core.area(siteId)
                    deps.core.challenges(roots, deps.routesJson(), listOfNotNull(area), deps.now())
                }
            mutableOutings.update { it.copy(challenges = ch, loading = false) }
        }
    }

    /** Imports the Strava archive the user picked (copied to [zip]) into the history, then computes again. */
    fun importStrava(zip: File) {
        mutableOutings.update { it.copy(importing = true, imported = null, importError = null) }
        viewModelScope.launch {
            val r = withContext(io) { runCatching { deps.core.importStrava(zip, deps.core.outingsRoot()) }.also { zip.delete() } }
            mutableOutings.update {
                it.copy(importing = false, imported = r.getOrNull(), importError = r.exceptionOrNull()?.message)
            }
            openOutings()
        }
    }

    companion object {
        private const val AROUND_DEG = 0.1

        /** A box of about 10 km around a position when no pack is open. */
        fun aroundBox(f: Fix): DoubleArray = doubleArrayOf(f.lon - AROUND_DEG, f.lat - AROUND_DEG, f.lon + AROUND_DEG, f.lat + AROUND_DEG)
    }
}

/** The OSM note (V12): drafted by the core at the position, sent when the user taps. */
class OsmViewModel(
    private val core: CommunityCore,
    private val notes: OsmNotes,
    host: String,
    private val french: Boolean,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutable = MutableStateFlow(OsmState(host = host))
    val state: StateFlow<OsmState> = mutable.asStateFlow()
    private var position: Fix? = null

    fun setPosition(fix: Fix?) {
        position = fix ?: position
        draft()
    }

    fun setCorrection(c: Correction) {
        mutable.update { it.copy(correction = c) }
        draft()
    }

    fun setComment(text: String) {
        mutable.update { it.copy(comment = text) }
        draft()
    }

    private fun draft() {
        val p = position ?: return
        val s = state.value
        val r = runCatching { core.osmNote(s.correction, null, null, s.comment, p.lon, p.lat, french) }
        mutable.update { it.copy(note = r.getOrNull(), error = r.exceptionOrNull()?.message, sent = null) }
    }

    /** Sends the note the user read. */
    fun send() {
        val note = state.value.note ?: return
        mutable.update { it.copy(sending = true) }
        viewModelScope.launch {
            val r = withContext(io) { notes.send(note) }
            mutable.update { it.copy(sending = false, sent = r.getOrNull(), error = r.exceptionOrNull()?.message) }
        }
    }
}
