package fr.skimap.group

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.security.SecureRandom

/** What the group screen shows. */
sealed interface GroupUi {
    data object Loading : GroupUi

    /** No group on this phone; [invalidQr]: the last QR scanned was not a group. */
    data class NoGroup(
        val invalidQr: Boolean = false,
    ) : GroupUi

    /** A group scanned: this phone's member is to be chosen. */
    data class Choosing(
        val info: GroupInfo,
    ) : GroupUi

    /** This phone is member [me] of the group. */
    data class InGroup(
        val info: GroupInfo,
        val me: Int,
        val qr: QrMatrix,
    ) : GroupUi
}

/**
 * The group screen's logic: create a group (the key from [SecureRandom], the plan of the
 * day if one is validated), read a scanned QR code, choose who one is, leave. Core calls on
 * [io].
 */
class GroupController(
    private val backend: GroupBackend,
    private val store: GroupStore,
    /** The plan of the day validated on this phone (its directory), if any. */
    private val planDir: () -> String?,
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val now: () -> Long = System::currentTimeMillis,
) {
    private val mutable = MutableStateFlow<GroupUi>(GroupUi.Loading)
    val ui: StateFlow<GroupUi> = mutable.asStateFlow()

    /** Whether a plan of the day would be shared by a group created now. */
    var hasPlan: Boolean = false
        private set

    fun load() {
        scope.launch {
            mutable.value =
                withContext(io) {
                    hasPlan = planDir() != null
                    val j = store.joined()
                    val info = j?.let { runCatching { backend.read(it.qrText) }.getOrNull() }
                    if (j != null && info != null) GroupUi.InGroup(info, j.me, backend.qr(j.qrText)) else GroupUi.NoGroup()
                }
        }
    }

    /** A new group: [names] with this phone's member first. */
    fun create(names: List<String>) {
        val members = names.map { fit(it.trim()) }.filter { it.isNotEmpty() }.take(MAX_MEMBERS)
        if (members.isEmpty()) return
        scope.launch {
            mutable.value =
                withContext(io) {
                    val key = ByteArray(KEY_BYTES).also { SecureRandom().nextBytes(it) }
                    // The core refuses what the form let through (a name of spaces only): no group.
                    runCatching { backend.create(key, members, planDir(), now() / MS) }.fold({ info ->
                        store.join(Joined(info.qrText, 0))
                        GroupUi.InGroup(info, 0, backend.qr(info.qrText))
                    }, { GroupUi.NoGroup() })
                }
        }
    }

    /** A name cut to the bytes a frame's group allows (UTF-8, whole characters). */
    private fun fit(name: String): String {
        var out = name
        while (out.toByteArray(Charsets.UTF_8).size > MAX_NAME_BYTES) out = out.dropLast(1)
        return out
    }

    /** A QR code scanned. */
    fun scanned(text: String) {
        scope.launch {
            mutable.value =
                withContext(io) {
                    runCatching { backend.read(text) }.fold({ GroupUi.Choosing(it) }, { GroupUi.NoGroup(invalidQr = true) })
                }
        }
    }

    /** This phone is member [me] of the group being joined. */
    fun choose(me: Int) {
        val c = mutable.value as? GroupUi.Choosing ?: return
        scope.launch {
            mutable.value =
                withContext(io) {
                    store.join(Joined(c.info.qrText, me))
                    GroupUi.InGroup(c.info, me, backend.qr(c.info.qrText))
                }
        }
    }

    fun leave() {
        scope.launch {
            withContext(io) { store.leave() }
            mutable.value = GroupUi.NoGroup()
        }
    }

    private companion object {
        const val KEY_BYTES = 32
        const val MS = 1000L
        const val MAX_MEMBERS = 16
        const val MAX_NAME_BYTES = 24
    }
}
