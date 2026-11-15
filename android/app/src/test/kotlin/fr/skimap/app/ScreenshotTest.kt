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
 * `recordRoborazziDebug` records new references. Light, dark and large font (×2).
 * The map itself (MapLibre, native) is replaced by a snow-white surface.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w412dp-h915dp-hdpi")
class ScreenshotTest {
    @get:Rule
    val rule = createComposeRule()

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

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
        rule.setContent { Themed { TestMapScreen(readyState(), mapMoved = true) } }
        capture("map_moved_light")
    }

    @Test
    fun mapLargeFont() {
        rule.setContent { Themed(fontScale = 2f) { TestMapScreen(readyState(), mapMoved = true) } }
        capture("map_large_font")
    }

    @Test
    fun sheetLight() {
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { MapSheetContent(readyState(), {}, {}, {}, TODAY, PARIS) } } }
        capture("sheet_light")
    }

    @Test
    fun sheetLayers() {
        val state = readyState().copy(hidden = setOf("no-network", "refuges"))
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { MapSheetContent(state, {}, {}, {}, TODAY, PARIS) } } }
        rule.onNodeWithText("Couches").performScrollTo()
        capture("sheet_layers")
    }

    @Test
    fun sheetDarkLargeFont() {
        rule.setContent {
            Themed(
                dark = true,
                fontScale = 2f,
            ) { Surface(Modifier.fillMaxSize()) { MapSheetContent(readyState(), {}, {}, {}, TODAY, PARIS) } }
        }
        capture("sheet_dark_large_font")
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
