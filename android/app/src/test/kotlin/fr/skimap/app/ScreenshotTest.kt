package fr.skimap.app

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.data.Practice
import fr.skimap.app.data.StyleChoice
import fr.skimap.app.ui.MapActions
import fr.skimap.app.ui.MapSheetContent
import fr.skimap.app.ui.MapUiState
import fr.skimap.app.ui.NoPackScreen
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Screen captures compared with the reference images in `src/test/screenshots`
 * (Roborazzi): `./gradlew :app:verifyRoborazziDebug` fails on any visual change,
 * `recordRoborazziDebug` records new references. French (light, dark, large font ×2) and
 * English. The map itself (MapLibre, native) is replaced by a snow-white surface; the
 * map drawn with the validated styles is captured on the emulator (README).
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class ScreenshotTest {
    @get:Rule
    val rule = createComposeRule()

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    private fun srn() =
        readyState().let { it.copy(opened = it.opened.copy(style = Fixtures.style("srn")), choice = StyleChoice(Practice.SRN)) }

    @Test
    fun mapLight() {
        rule.setContent { Themed { TestMapScreen(readyState()) } }
        capture("map_light")
    }

    @Test
    fun mapDark() {
        rule.setContent { Themed(dark = true) { TestMapScreen(readyState()) } }
        capture("map_dark")
    }

    @Test
    fun mapMovedLight() {
        rule.setContent { Themed { TestMapScreen(readyState(free = true), mapMoved = true) } }
        capture("map_moved_light")
    }

    @Test
    fun mapLargeFont() {
        rule.setContent { Themed(fontScale = 2f) { TestMapScreen(readyState(), mapMoved = true) } }
        capture("map_large_font")
    }

    @Test
    fun sheetLight() {
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { MapSheetContent(readyState(), MapActions(), {}, TODAY, PARIS) } } }
        capture("sheet_light")
    }

    @Test
    fun sheetLayers() {
        val state = srn().copy(hidden = setOf("reseau", "refuges"))
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { MapSheetContent(state, MapActions(), {}, TODAY, PARIS) } } }
        rule.onNodeWithText("Couches").performScrollTo()
        capture("sheet_layers")
    }

    @Test
    fun sheetLegendSrn() {
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { MapSheetContent(srn(), MapActions(), {}, TODAY, PARIS) } } }
        rule.onNodeWithText("Très exposé").performScrollTo()
        capture("sheet_legend_srn")
    }

    @Test
    fun sheetDarkLargeFont() {
        rule.setContent {
            Themed(dark = true, fontScale = 2f) {
                Surface(Modifier.fillMaxSize()) { MapSheetContent(readyState(), MapActions(), {}, TODAY, PARIS) }
            }
        }
        capture("sheet_dark_large_font")
    }

    /** Free mode over the France basemap alone, no pack installed (X20, #162): the header says so. */
    @Test
    fun mapFranceOnly() {
        val ready = readyState(free = true)
        val opened = Fixtures.opened("", "France, aucun domaine installé").copy(packCount = 0)
        rule.setContent { Themed { TestMapScreen(ready.copy(packs = emptyList(), opened = opened)) } }
        rule.onNodeWithText("France · aucun domaine installé").assertExists()
        capture("map_france_only")
    }

    @Test
    fun noPack() {
        rule.setContent {
            Themed {
                NoPackScreen(
                    MapUiState.NoPack(
                        "/storage/emulated/0/Android/data/fr.skimap.app/files/packs",
                        listOf(Fixtures.entry("/packs/vieux", "vieux", error = "format version 2 not supported")),
                    ),
                    onRetry = {},
                )
            }
        }
        capture("no_pack")
    }
}

/** English captures (VO6): the app on a phone that is not in French. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "en-w412dp-h915dp-hdpi")
class EnglishScreenshotTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun mapEnglish() {
        rule.setContent { Themed { TestMapScreen(readyState(free = true)) } }
        rule.onRoot().captureRoboImage("src/test/screenshots/map_en.png")
    }

    @Test
    fun sheetEnglish() {
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { MapSheetContent(readyState(), MapActions(), {}, TODAY, PARIS) } } }
        rule.onRoot().captureRoboImage("src/test/screenshots/sheet_en.png")
    }
}
