package fr.skimap.app

import android.app.Application
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import fr.skimap.app.sync.LegacyVoice
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.io.File

/** What an earlier version kept for the pre-generated voice is removed at the update, nothing else. */
@RunWith(RobolectricTestRunner::class)
class LegacyVoiceTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()

    @Test
    fun theVoicePacksAndTheSavedVolumeOfAnEarlierVersionAreRemoved() {
        val root = app.getExternalFilesDir(null)!!
        val clip = File(root, "voice/test/en/voice/turn.left.200.opus").apply { parentFile!!.mkdirs() }
        clip.writeBytes(ByteArray(16))
        val pack = File(root, "packs/test/manifest.json").apply { parentFile!!.mkdirs() }
        pack.writeText("{}")
        // The volume an earlier version kept to give back: dropped, never applied.
        app
            .getSharedPreferences(LegacyVoice.VOLUME_PREFS, Context.MODE_PRIVATE)
            .edit()
            .putInt("stream", 7)
            .commit()
        assertTrue(LegacyVoice.cleanUp(app))
        assertFalse(File(root, "voice").exists())
        assertTrue(pack.isFile)
        assertEquals(0, app.getSharedPreferences(LegacyVoice.VOLUME_PREFS, Context.MODE_PRIVATE).all.size)
        // Nothing left: nothing done.
        assertFalse(LegacyVoice.cleanUp(app))
    }
}
