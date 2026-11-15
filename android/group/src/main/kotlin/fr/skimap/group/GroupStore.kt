package fr.skimap.group

import android.content.Context
import androidx.core.content.edit

/** The group of this phone: its QR text and this phone's member number. */
data class Joined(
    val qrText: String,
    val me: Int,
)

/** Where the group is kept (private preferences; the key never leaves the phone but by the QR code). */
class GroupStore(
    private val context: Context,
    /** Preferences file (two phones simulated in one process in the instrumented test). */
    private val name: String = PREFS,
) {
    private fun prefs() = context.getSharedPreferences(name, Context.MODE_PRIVATE)

    fun joined(): Joined? {
        val p = prefs()
        val qr = p.getString(KEY_QR, null) ?: return null
        val me = p.getInt(KEY_ME, -1)
        return if (me >= 0) Joined(qr, me) else null
    }

    fun join(j: Joined) {
        prefs().edit {
            putString(KEY_QR, j.qrText)
            putInt(KEY_ME, j.me)
        }
    }

    fun leave() {
        prefs().edit { clear() }
    }

    private companion object {
        const val PREFS = "skimap_group"
        const val KEY_QR = "qr"
        const val KEY_ME = "me"
    }
}
