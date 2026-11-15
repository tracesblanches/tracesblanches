package fr.skimap.app.ui

import androidx.activity.compose.ReportDrawnWhen
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.skimap.app.R
import fr.skimap.app.ui.domains.DomainPickerDialog
import fr.skimap.app.ui.domains.DomainsViewModel
import fr.skimap.app.ui.map.MapController
import fr.skimap.app.ui.map.MapEngine
import fr.skimap.app.ui.map.MapProbe
import fr.skimap.app.ui.map.PackMap
import fr.skimap.app.ui.map.styleName
import fr.skimap.core.Card
import fr.skimap.core.PackInfo
import java.time.LocalDate
import java.time.ZoneId

/** Test tags of the main screen. */
object Tags {
    const val MAP = "map"
    const val HEADER = "header"
    const val RECENTER = "recenter"
    const val PRIMARY = "primary_action"
    const val ATTRIBUTION = "attribution"
    const val SHEET = "sheet"
    const val CARD = "card"
}

/** Root of the UI: one screen, driven by [viewModel]. */
@Composable
fun SkimapApp(
    viewModel: MapViewModel,
    domains: DomainsViewModel,
) {
    var pickerOpen by rememberSaveable { mutableStateOf(false) }
    var card by remember { mutableStateOf<Card?>(null) }
    card?.let { CardSheet(it, onDismiss = { card = null }) }
    if (pickerOpen) {
        DomainPickerDialog(domains, onPick = viewModel::selectPack, onClose = { pickerOpen = false })
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val engineReady by MapEngine.ready.collectAsStateWithLifecycle()
    val rendered by MapProbe.rendered.collectAsStateWithLifecycle()
    val today = LocalDate.now()
    val zone = ZoneId.systemDefault()
    val mapShownFor = rendered?.styleName
    // Fully drawn (startup benchmark, TTFD): the map of the pack rendered, or a final screen.
    ReportDrawnWhen {
        when (val s = state) {
            MapUiState.Loading -> false
            is MapUiState.Ready -> mapShownFor == styleName(s.opened)
            else -> true
        }
    }
    when (val s = state) {
        MapUiState.Loading -> {
            LoadingScreen()
        }

        is MapUiState.NoPack -> {
            NoPackScreen(s, onRetry = viewModel::reload, onChooseDomain = { pickerOpen = true })
        }

        is MapUiState.Failed -> {
            FailedScreen(s, onSelectPack = viewModel::selectPack, onRetry = viewModel::reload)
        }

        is MapUiState.Ready -> {
            val controller = remember { MapController() }
            val moved by controller.moved.collectAsStateWithLifecycle()
            // The map lambda captures only the opened pack: an error message or the sheet
            // does not recompose the map (counted in MapScreenTest).
            val opened = s.opened
            val hidden = s.hidden
            MapScreen(
                state = s,
                today = today,
                zone = zone,
                mapShown = mapShownFor == styleName(opened),
                mapMoved = moved,
                actions =
                    MapActions(
                        recenter = controller::recenter,
                        setLayerVisible = viewModel::setLayerVisible,
                        chooseDomain = { pickerOpen = true },
                        selectFree = viewModel::selectFree,
                        selectBasemap = viewModel::selectBasemap,
                    ),
            ) {
                if (engineReady) {
                    PackMap(
                        opened = opened,
                        controller = controller,
                        modifier = Modifier.fillMaxSize(),
                        hidden = hidden,
                        onCard = { card = it },
                    )
                }
            }
        }
    }
}

/** Callbacks of [MapScreen]. */
data class MapActions(
    val recenter: () -> Unit = {},
    val chooseDomain: () -> Unit = {},
    val selectFree: () -> Unit = {},
    val selectBasemap: (String) -> Unit = {},
    val setLayerVisible: (String, Boolean) -> Unit = { _, _ -> },
)

/**
 * Main screen: the map first, edge to edge. A one-line status chip at the top, and in the
 * bottom corners small semi-transparent controls of standard Material size (48 dp
 * targets): the map and legend sheet (primary action), "Vue auto" only once the user has
 * moved the map, and a small ⓘ that opens the full data attributions. The map takes the
 * usual gestures (pinch, double tap, double tap and slide with one finger); with gloves,
 * the automatic view, physical controls (K13) and voice replace big on-screen buttons.
 */
@Composable
fun MapScreen(
    state: MapUiState.Ready,
    today: LocalDate,
    zone: ZoneId,
    actions: MapActions,
    modifier: Modifier = Modifier,
    mapShown: Boolean = true,
    mapMoved: Boolean = false,
    initialSheetOpen: Boolean = false,
    map: @Composable () -> Unit,
) {
    var sheetOpen by rememberSaveable { mutableStateOf(initialSheetOpen) }
    var creditsOpen by rememberSaveable { mutableStateOf(false) }
    val info = state.opened.info
    Box(modifier.fillMaxSize()) {
        val basemapTitle =
            info.basemaps
                .firstOrNull { it.id == state.opened.style.basemap }
                ?.title
                .orEmpty()
        val mapDescription =
            stringResource(
                if (mapShown) R.string.map_shown else R.string.map_loading,
                info.siteName,
                basemapTitle,
            )
        Box(Modifier.fillMaxSize().testTag(Tags.MAP).semantics { contentDescription = mapDescription }) { map() }
        // Status bar over the light map: a band of the theme surface keeps its icons
        // readable in both themes (edge to edge).
        Box(
            Modifier
                .fillMaxWidth()
                .windowInsetsTopHeight(WindowInsets.statusBars)
                .background(MaterialTheme.colorScheme.surface.copy(alpha = CONTROL_ALPHA)),
        )
        if (!mapShown) {
            // Static (no animation): MapLibre draws its first frame behind it.
            Chip(Modifier.align(Alignment.Center)) {
                Text(stringResource(R.string.map_drawing), style = MaterialTheme.typography.bodyMedium)
            }
        }
        Box(
            Modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(8.dp),
        ) {
            StatusChip(info, today, state.error, Modifier.align(Alignment.TopStart))
            Column(
                Modifier.align(Alignment.BottomEnd),
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (mapMoved) {
                    SmallControl(
                        painterResource(R.drawable.ic_recenter),
                        stringResource(R.string.recenter, info.siteName),
                        actions.recenter,
                        Modifier.testTag(Tags.RECENTER),
                    )
                }
                SmallControl(
                    painterResource(R.drawable.ic_layers),
                    stringResource(R.string.map_and_legend),
                    { sheetOpen = true },
                    Modifier.testTag(Tags.PRIMARY),
                )
            }
            IconButton(
                onClick = { creditsOpen = true },
                modifier = Modifier.align(Alignment.BottomStart).size(48.dp).testTag(Tags.ATTRIBUTION),
            ) {
                Surface(
                    shape = CircleShape,
                    color = MaterialTheme.colorScheme.surface.copy(alpha = CONTROL_ALPHA),
                    contentColor = MaterialTheme.colorScheme.onSurface,
                ) {
                    Icon(
                        painterResource(R.drawable.ic_info),
                        contentDescription = stringResource(R.string.credits),
                        modifier = Modifier.size(20.dp),
                    )
                }
            }
        }
    }
    if (sheetOpen) {
        MapSheet(
            state = state,
            today = today,
            zone = zone,
            onChooseDomain = {
                sheetOpen = false
                actions.chooseDomain()
            },
            onSelectFree = actions.selectFree,
            onSelectBasemap = actions.selectBasemap,
            onSetLayerVisible = actions.setLayerVisible,
            onDismiss = { sheetOpen = false },
        )
    }
    if (creditsOpen) {
        AlertDialog(
            onDismissRequest = { creditsOpen = false },
            confirmButton = { TextButton(onClick = { creditsOpen = false }) { Text(stringResource(R.string.close)) } },
            title = { Text(stringResource(R.string.sheet_sources)) },
            text = {
                Column(
                    Modifier.verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    state.opened.style.attributions
                        .forEach { Text(it, style = MaterialTheme.typography.bodyMedium) }
                }
            },
        )
    }
}

/** Translucent rounded surface of the map overlays. */
@Composable
private fun Chip(
    modifier: Modifier = Modifier,
    color: Color = MaterialTheme.colorScheme.surface.copy(alpha = CONTROL_ALPHA),
    contentColor: Color = MaterialTheme.colorScheme.onSurface,
    content: @Composable () -> Unit,
) {
    Surface(modifier = modifier, shape = RoundedCornerShape(12.dp), color = color, contentColor = contentColor) {
        Box(Modifier.padding(horizontal = 10.dp, vertical = 4.dp)) { content() }
    }
}

/**
 * One line at the top: site and piste status. The validity shows only when it matters
 * (last day, expired), in words and on the error colour.
 */
@Composable
private fun StatusChip(
    info: PackInfo,
    today: LocalDate,
    error: String?,
    modifier: Modifier = Modifier,
) {
    val validity = PackTexts.validity(info, today)
    val warn = validity == Validity.EXPIRED || validity == Validity.LAST_DAY
    Column(modifier.testTag(Tags.HEADER), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Chip {
            Text(
                "${info.siteName} · ${PackTexts.statusShort(info.status)}",
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading() },
            )
        }
        val alert = error ?: if (warn) PackTexts.validityText(info, today) else null
        if (alert != null) {
            Chip(color = MaterialTheme.colorScheme.errorContainer, contentColor = MaterialTheme.colorScheme.onErrorContainer) {
                Text(alert, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** Standard-size (48 dp) translucent map control. */
@Composable
fun SmallControl(
    painter: Painter,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.size(48.dp),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface.copy(alpha = CONTROL_ALPHA),
        contentColor = MaterialTheme.colorScheme.onSurface,
        shadowElevation = 2.dp,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Icon(painter, contentDescription = contentDescription, modifier = Modifier.size(24.dp))
        }
    }
}

private const val CONTROL_ALPHA = 0.88f
