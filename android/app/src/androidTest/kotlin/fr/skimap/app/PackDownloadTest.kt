package fr.skimap.app

import android.content.Context
import android.content.Intent
import android.os.ParcelFileDescriptor
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ActivityScenario
import androidx.test.espresso.Espresso
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.WorkInfo
import androidx.work.WorkManager
import fr.skimap.app.sync.PackSync
import fr.skimap.app.sync.PackSyncWorker
import fr.skimap.app.ui.domains.DomainTags
import fr.skimap.app.ui.map.MapProbe
import fr.skimap.core.listPacks
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/**
 * Download and update of packs (K2) on a device or emulator, through the UI and
 * WorkManager, from a local skimap-server serving the test store (`./gradlew
 * :app:testStore`; instrumentation argument `skimapServer`, e.g. `http://10.0.2.2:8787`,
 * android/docs/verifier.md): first download, differential update, sync without change,
 * airplane mode. Without the argument, skipped.
 */
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalTestApi::class)
class PackDownloadTest {
    @get:Rule
    val rule = createEmptyComposeRule()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val target: Context = instrumentation.targetContext
    private val packs = File(target.getExternalFilesDir(null), "packs")
    private val server: String? = InstrumentationRegistry.getArguments().getString("skimapServer")
    private val wm get() = WorkManager.getInstance(target)

    private fun shell(cmd: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(cmd)).use {
            it.readBytes().decodeToString()
        }

    private fun copyAssets(
        path: String,
        dest: File,
    ) {
        val assets = instrumentation.context.assets
        val children = assets.list(path).orEmpty()
        if (children.isEmpty()) {
            dest.parentFile?.mkdirs()
            assets.open(path).use { input -> dest.outputStream().use { input.copyTo(it) } }
        } else {
            children.forEach { copyAssets("$path/$it", File(dest, it)) }
        }
    }

    @Before
    fun setUp() {
        assumeTrue("instrumentation argument skimapServer not given: no local server", server != null)
        // The local server is on a private address (Android 17 local network protection);
        // the permission exists in debug builds only. Older Android: no such permission.
        if (android.os.Build.VERSION.SDK_INT >= LOCAL_NETWORK_SDK) {
            instrumentation.uiAutomation.grantRuntimePermission(target.packageName, "android.permission.ACCESS_LOCAL_NETWORK")
        }
        shell("cmd connectivity airplane-mode disable")
        shell("cmd uimode night no")
        wm.cancelAllWork().result.get()
        wm.pruneWork().result.get()
        packs.deleteRecursively()
        packs.mkdirs()
        File(target.filesDir, "server").deleteRecursively()
        target
            .getSharedPreferences("skimap", Context.MODE_PRIVATE)
            .edit()
            .clear()
            .putString("server_url", server)
            // No daily work during the tests: only the works the tests start.
            .putBoolean("auto_update", false)
            .commit()
    }

    @After
    fun tearDown() {
        shell("cmd connectivity airplane-mode disable")
    }

    /** Waits (bounded) for a text on screen; a capture of the screen if it never comes. */
    private fun waitText(
        text: String,
        timeoutMs: Long = TIMEOUT_MS,
    ) {
        try {
            rule.waitUntil(timeoutMs) { rule.onAllNodes(hasText(text, substring = true)).fetchSemanticsNodes().isNotEmpty() }
        } catch (e: androidx.compose.ui.test.ComposeTimeoutException) {
            shell("screencap -p /data/local/tmp/skimap-download-failure.png")
            throw AssertionError("no \"$text\" on screen (capture: /data/local/tmp/skimap-download-failure.png)", e)
        }
    }

    private fun launch(): ActivityScenario<MainActivity> = ActivityScenario.launch(Intent(target, MainActivity::class.java))

    private fun installedVersion(): String? = listPacks(packs.absolutePath).firstOrNull { it.siteId == "test" }?.packVersion

    private fun work(site: String = "test"): WorkInfo? = wm.getWorkInfosForUniqueWork(PackSync.workName(site)).get().lastOrNull()

    private fun logSync(
        what: String,
        site: String = "test",
    ) {
        val o = checkNotNull(work(site)).outputData
        android.util.Log.i(
            "skimap-test",
            "$what: ${o.getString(PackSyncWorker.FROM)} -> ${o.getString(PackSyncWorker.TO)}, " +
                "${o.getInt(PackSyncWorker.REQUESTS, -1)} requests, ${o.getLong(PackSyncWorker.RECEIVED, -1)} bytes, " +
                "${o.getLong(PackSyncWorker.MILLIS, -1)} ms",
        )
    }

    /** Clicks the first [label] of the list of areas, keyboard closed, scrolled to it. */
    private fun clickInList(label: String) {
        waitText(label)
        Espresso.closeSoftKeyboard()
        rule.onNodeWithTag(DomainTags.LIST).performScrollToNode(hasText(label, substring = true))
        rule.onAllNodes(hasText(label, substring = true))[0].performClick()
    }

    /** The area picker, the Cervières area (covered by the test site), its button. */
    private fun pickerButton(label: String) {
        rule.onNodeWithText(target.getString(R.string.choose_domain)).performClick()
        rule.onNodeWithTag(DomainTags.SEARCH).performTextInput("Cervières")
        clickInList(label)
    }

    @Test
    fun downloadFromTheServerThenTheMap() {
        launch().use {
            // No pack: the picker lists the areas the server publishes.
            pickerButton(target.getString(R.string.domain_download, ""))
            rule.waitUntil(TIMEOUT_MS) { work()?.state == WorkInfo.State.SUCCEEDED }
            logSync("first download")
            assertEquals("20261116T060000Z", installedVersion())
            // The map opens by itself on the new pack, in the validated style.
            rule.onNodeWithText(target.getString(R.string.close)).performClick()
            rule.waitUntil(TIMEOUT_MS) { MapProbe.rendered.value?.styleName == "skimap libre fond-clair" }
        }
    }

    @Test
    fun differentialUpdateAndNothingNew() {
        copyAssets("testpack", File(packs, "test"))
        assertEquals("20261115T060000Z", installedVersion())
        launch().use {
            rule.waitUntil(TIMEOUT_MS) { MapProbe.rendered.value?.styleName == "skimap libre fond-clair" }
            rule.onNodeWithTag(fr.skimap.app.ui.Tags.PRIMARY).performClick()
            rule.onNodeWithText(target.getString(R.string.choose_domain)).performClick()
            rule.onNodeWithTag(DomainTags.SEARCH).performTextInput("Cervières")
            clickInList(target.getString(R.string.domain_update))
            rule.waitUntil(TIMEOUT_MS) { work()?.state == WorkInfo.State.SUCCEEDED }
            logSync("differential update")
            assertEquals("20261116T060000Z", installedVersion())
            assertEquals("20261115T060000Z", work()!!.outputData.getString(PackSyncWorker.FROM))
            // Nothing new on the server: one request, 304, no body.
            PackSync.download(target, "test", "/v1/sites/test/index.json", 1)
            rule.waitUntil(TIMEOUT_MS) { work()?.state == WorkInfo.State.SUCCEEDED }
            logSync("sync without change")
            val o = work()!!.outputData
            assertEquals(1, o.getInt(PackSyncWorker.REQUESTS, -1))
            assertEquals(0L, o.getLong(PackSyncWorker.RECEIVED, -1))
        }
    }

    @Test
    fun airplaneModeLosesNothing() {
        copyAssets("testpack", File(packs, "test"))
        // Read the server catalog once with the network, as the picker does.
        launch().use {
            rule.waitUntil(TIMEOUT_MS) { MapProbe.rendered.value?.styleName == "skimap libre fond-clair" }
        }
        val catalog = PackSync.catalog(target)
        assertTrue(checkNotNull(catalog.refresh()).any { it.siteId == "test" })
        shell("cmd connectivity airplane-mode enable")
        launch().use {
            // The map, its layers and cards work without any network.
            rule.waitUntil(TIMEOUT_MS) { MapProbe.rendered.value?.styleName == "skimap libre fond-clair" }
            rule.onNodeWithTag(fr.skimap.app.ui.Tags.PRIMARY).performClick()
            rule.onNodeWithText(target.getString(R.string.choose_domain)).performClick()
            // Offline: the last server list, said; the update waits for a network.
            waitText(target.getString(R.string.domains_offline))
            rule.onNodeWithTag(DomainTags.SEARCH).performTextInput("Cervières")
            clickInList(target.getString(R.string.domain_update))
            val waiting = target.getString(R.string.domain_waiting_network)
            waitText(waiting)
            // Still waiting a while later; the pack in place is untouched.
            Thread.sleep(OFFLINE_MS)
            assertEquals(WorkInfo.State.ENQUEUED, work()?.state)
            assertEquals("20261115T060000Z", installedVersion())
            shell("cmd connectivity airplane-mode disable")
            rule.waitUntil(NETWORK_BACK_MS) { work()?.state == WorkInfo.State.SUCCEEDED }
            logSync("update after airplane mode")
            assertEquals("20261116T060000Z", installedVersion())
        }
    }

    /**
     * The case that dominates: a real pack (Cervières, 45 MB) from a local server where the
     * pipeline's pack and a next-day version are published (instrumentation argument
     * `skimapRealServer`, android/docs/verifier.md): first download, sync without change,
     * daily delta, timed on the device through WorkManager, the core and the disk.
     */
    @Test
    fun realPackDownloadMeasured() {
        val real = InstrumentationRegistry.getArguments().getString("skimapRealServer")
        assumeTrue("instrumentation argument skimapRealServer not given", real != null)
        val src = "/data/local/tmp/skimap-packs/cervieres"
        assumeTrue("needs the pipeline's pack in $src", shell("ls $src/manifest.json").contains("manifest.json"))
        target
            .getSharedPreferences("skimap", Context.MODE_PRIVATE)
            .edit()
            .putString("server_url", real)
            .commit()
        val index = "/v1/sites/cervieres/index.json"

        fun run(what: String) {
            PackSync.download(target, "cervieres", index, 1, mobileData = true)
            // Bounded: 45 MB over the emulator network, installed by the core.
            val deadline = System.currentTimeMillis() + REAL_TIMEOUT_MS
            while (work("cervieres")?.state?.isFinished != true) {
                check(System.currentTimeMillis() < deadline) { "$what: not finished" }
                Thread.sleep(POLL_MS)
            }
            assertEquals(WorkInfo.State.SUCCEEDED, work("cervieres")?.state)
            logSync(what, "cervieres")
        }
        run("real first download")
        assertEquals("20261116T060000Z", listPacks(packs.absolutePath).single().packVersion)
        run("real sync without change")
        assertEquals(1, work("cervieres")!!.outputData.getInt(PackSyncWorker.REQUESTS, -1))
        // The pipeline's pack (version 1) copied, no state: the next-day delta only.
        File(packs, "cervieres").deleteRecursively()
        File(packs, ".cervieres.sync.json").delete()
        shell("cp -r $src " + File(packs, "cervieres").absolutePath)
        run("real daily delta")
        assertEquals(2, work("cervieres")!!.outputData.getInt(PackSyncWorker.REQUESTS, -1))
        assertEquals("20261116T060000Z", listPacks(packs.absolutePath).single().packVersion)
    }

    private companion object {
        /** Android 17: local network protection. */
        const val LOCAL_NETWORK_SDK = 37
        const val REAL_TIMEOUT_MS = 600_000L
        const val POLL_MS = 200L
        const val TIMEOUT_MS = 60_000L
        const val OFFLINE_MS = 5_000L
        const val NETWORK_BACK_MS = 120_000L
    }
}
