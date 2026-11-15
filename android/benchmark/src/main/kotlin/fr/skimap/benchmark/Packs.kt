package fr.skimap.benchmark

import android.content.Intent
import androidx.benchmark.macro.MacrobenchmarkScope
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until
import java.util.regex.Pattern

const val PACKAGE = "fr.skimap.app"
const val SITE = "cervieres"
private const val SOURCE = "/data/local/tmp/skimap-packs"
private const val RENDER_TIMEOUT_MS = 120_000L

/** Texts of the app in French and English (the language of the device). */
private val NO_PACK: Pattern = Pattern.compile("Aucun pack sur ce téléphone|No pack on this phone")
private val MAP_SHOWN: Pattern = Pattern.compile(".*(affichée|shown)")

/**
 * Copies the pack of [SITE] into the app storage once: the app creates its own directory
 * at its first start (a directory made by the shell would not be readable by the app).
 */
fun MacrobenchmarkScope.installPack() {
    val dest = "/sdcard/Android/data/$PACKAGE/files/packs"
    if (device.executeShellCommand("ls $dest/$SITE/manifest.json").contains("manifest.json")) return
    check(device.executeShellCommand("ls $SOURCE/$SITE/manifest.json").contains("manifest.json")) {
        "push a pack to $SOURCE/$SITE first (README)"
    }
    startActivityAndWait()
    // The app has created its packs directory once it says there is no pack.
    check(device.wait(Until.hasObject(By.text(NO_PACK)), RENDER_TIMEOUT_MS)) { "no empty screen" }
    device.executeShellCommand("cp -r $SOURCE/$SITE $dest/$SITE")
    killProcess()
}

/** Starts the app on [SITE] and waits until the map is drawn (its description says so). */
fun MacrobenchmarkScope.startOnPackAndWaitForMap() {
    startActivityAndWait(Intent().setClassName(PACKAGE, "$PACKAGE.MainActivity").putExtra("pack", SITE))
    check(device.wait(Until.hasObject(By.desc(MAP_SHOWN)), RENDER_TIMEOUT_MS)) { "map not drawn" }
}
