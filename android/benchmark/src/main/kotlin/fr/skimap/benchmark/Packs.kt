package fr.skimap.benchmark

import android.content.Intent
import androidx.benchmark.macro.MacrobenchmarkScope
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until
import java.util.regex.Pattern

const val PACKAGE = "fr.skimap.app"
private const val SOURCE = "/data/local/tmp/skimap-packs"
private const val RENDER_TIMEOUT_MS = 120_000L

/** Texts of the app in French and English (the language of the device). */
private val NO_PACK: Pattern = Pattern.compile("Aucun pack sur ce téléphone|No pack on this phone")
private val MAP_SHOWN: Pattern = Pattern.compile(".*(affichée|shown)")

/** Site of the benchmark: the first pack pushed to [SOURCE] (any domain). */
private fun MacrobenchmarkScope.benchSite(): String =
    device
        .executeShellCommand("ls $SOURCE")
        .lineSequence()
        .map { it.trim() }
        .firstOrNull { it.isNotEmpty() && !it.contains("No such file") }
        ?: error("push a pack to $SOURCE/<site> first (README)")

/**
 * Copies the pack of the benchmark site into the app storage once: the app creates its own
 * directory at its first start (a directory made by the shell would not be readable by the app).
 */
fun MacrobenchmarkScope.installPack() {
    val site = benchSite()
    val dest = "/sdcard/Android/data/$PACKAGE/files/packs"
    if (device.executeShellCommand("ls $dest/$site/manifest.json").contains("manifest.json")) return
    check(device.executeShellCommand("ls $SOURCE/$site/manifest.json").contains("manifest.json")) {
        "push a pack to $SOURCE/$site first (README)"
    }
    startActivityAndWait()
    // The app has created its packs directory once it says there is no pack.
    check(device.wait(Until.hasObject(By.text(NO_PACK)), RENDER_TIMEOUT_MS)) { "no empty screen" }
    device.executeShellCommand("cp -r $SOURCE/$site $dest/$site")
    killProcess()
}

/** Starts the app on the benchmark site and waits until the map is drawn (its description says so). */
fun MacrobenchmarkScope.startOnPackAndWaitForMap() {
    startActivityAndWait(Intent().setClassName(PACKAGE, "$PACKAGE.MainActivity").putExtra("pack", benchSite()))
    check(device.wait(Until.hasObject(By.desc(MAP_SHOWN)), RENDER_TIMEOUT_MS)) { "map not drawn" }
}
