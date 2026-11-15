package fr.skimap.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import fr.skimap.app.ui.MapActions
import fr.skimap.app.ui.MapScreen
import fr.skimap.app.ui.MapUiState
import fr.skimap.app.ui.theme.SkimapTheme
import java.time.LocalDate
import java.time.ZoneId

/** Fixed day and zone: captures do not depend on the machine. */
val TODAY: LocalDate = LocalDate.of(2026, 11, 15)
val PARIS: ZoneId = ZoneId.of("Europe/Paris")

fun readyState(error: String? = null) =
    MapUiState.Ready(
        packs =
            listOf(
                Fixtures.entry(Fixtures.CERVIERES, "Cervières"),
                Fixtures.entry(Fixtures.CHAPELLE, "Chapelle-des-Bois"),
            ),
        opened = Fixtures.opened(),
        openMillis = 196,
        error = error,
    )

/** Stand-in for the MapLibre map (native, not available on the JVM): snow white. */
@Composable
fun FakeMap() {
    Box(Modifier.fillMaxSize().background(Color(0xFFF4F6F8)))
}

/** The app theme, optionally dark, with a font scale (Android setting "font size"). */
@Composable
fun Themed(
    dark: Boolean = false,
    fontScale: Float = 1f,
    content: @Composable () -> Unit,
) {
    val d = LocalDensity.current
    CompositionLocalProvider(LocalDensity provides Density(d.density, fontScale)) {
        SkimapTheme(darkTheme = dark, content = content)
    }
}

@Composable
fun TestMapScreen(
    state: MapUiState.Ready,
    actions: MapActions = MapActions(),
    initialSheetOpen: Boolean = false,
    mapMoved: Boolean = false,
    map: @Composable () -> Unit = { FakeMap() },
) {
    MapScreen(
        state = state,
        today = TODAY,
        zone = PARIS,
        actions = actions,
        mapMoved = mapMoved,
        initialSheetOpen = initialSheetOpen,
        map = map,
    )
}
