package fr.skimap.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.graphics.toColorInt
import fr.skimap.app.R
import fr.skimap.app.ui.map.SymbolBitmaps
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.LegendEntry
import fr.skimap.core.MapSymbol
import java.time.LocalDate
import java.time.ZoneId

/** The primary action: site, basemap, legend and sources, in a bottom sheet. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MapSheet(
    state: MapUiState.Ready,
    today: LocalDate,
    zone: ZoneId,
    onChooseDomain: () -> Unit,
    onSelectBasemap: (String) -> Unit,
    onSetLayerVisible: (String, Boolean) -> Unit,
    onDismiss: () -> Unit,
    onSelectFree: () -> Unit = {},
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        modifier = Modifier.testTag(Tags.SHEET),
        // The handle is a touch target too: 64 dp wide instead of 32 (accessibility check).
        dragHandle = { BottomSheetDefaults.DragHandle(width = 64.dp) },
    ) {
        MapSheetContent(
            state,
            onChooseDomain,
            onSelectBasemap,
            onDismiss,
            today = today,
            zone = zone,
            onSetLayerVisible = onSetLayerVisible,
            onSelectFree = onSelectFree,
        )
    }
}

/** Content of [MapSheet], separate so that it can be captured and tested alone. */
@Composable
fun MapSheetContent(
    state: MapUiState.Ready,
    onChooseDomain: () -> Unit,
    onSelectBasemap: (String) -> Unit,
    onClose: () -> Unit,
    today: LocalDate,
    zone: ZoneId,
    modifier: Modifier = Modifier,
    onSetLayerVisible: (String, Boolean) -> Unit = { _, _ -> },
    onSelectFree: () -> Unit = {},
) {
    val info = state.opened.info
    val style = state.opened.style
    Column(
        modifier
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            stringResource(R.string.sheet_title),
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.semantics { heading() },
        )
        // Pack: piste status with its time, validity.
        Text(PackTexts.statusText(info.status, zone), style = MaterialTheme.typography.bodyLarge)
        Text(PackTexts.validityText(info, today), style = MaterialTheme.typography.bodyMedium)
        SectionTitle(stringResource(R.string.sheet_site))
        Column(Modifier.selectableGroup()) {
            ChoiceRow(stringResource(R.string.mode_free), selected = state.free) { onSelectFree() }
            if (!state.free) ChoiceRow(stringResource(R.string.mode_area, info.siteName), selected = true) {}
        }
        OutlinedButton(onClick = onChooseDomain, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.choose_domain), style = MaterialTheme.typography.labelLarge)
        }
        if (info.basemaps.isNotEmpty()) {
            SectionTitle(stringResource(R.string.sheet_basemap))
            Column(Modifier.selectableGroup()) {
                info.basemaps.forEach { b ->
                    ChoiceRow(b.title, selected = b.id == style.basemap) { onSelectBasemap(b.id) }
                }
            }
        }
        if (style.toggles.isNotEmpty()) {
            SectionTitle(stringResource(R.string.sheet_layers))
            style.toggles.forEach { t ->
                SwitchRow(t.label, checked = t.id !in state.hidden) { onSetLayerVisible(t.id, it) }
            }
            // Safety elements cannot be hidden: a piste closed or one-way must stay visible.
            Text(stringResource(R.string.layers_always), style = MaterialTheme.typography.bodyMedium)
        }
        SectionTitle(stringResource(R.string.sheet_legend))
        Legend(state.opened.legend, state.opened.symbols)
        SectionTitle(stringResource(R.string.sheet_sources))
        style.attributions.forEach { Text(it, style = MaterialTheme.typography.bodyMedium) }
        if (info.personalSources) {
            Text(stringResource(R.string.personal_sources), style = MaterialTheme.typography.bodyMedium)
        }
        Text(
            stringResource(R.string.sheet_pack, info.packVersion, state.openMillis, state.opened.coreVersion),
            style = MaterialTheme.typography.bodySmall,
        )
        Button(onClick = onClose, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.close), style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun SectionTitle(text: String) {
    HorizontalDivider(thickness = 2.dp, color = MaterialTheme.colorScheme.outline)
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
}

/** A radio row of at least [GloveTarget]: the whole row is the target. */
@Composable
private fun ChoiceRow(
    label: String,
    selected: Boolean,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = GloveTarget)
            .selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null, enabled = enabled, modifier = Modifier.size(40.dp))
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 12.dp))
    }
}

/** A switch row of at least [GloveTarget]: the whole row is the target. */
@Composable
private fun SwitchRow(
    label: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = GloveTarget)
            .toggleable(value = checked, role = Role.Switch, onValueChange = onChange),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = null)
    }
}

@Composable
private fun Legend(
    entries: List<LegendEntry>,
    symbols: List<MapSymbol>,
) {
    val density = LocalDensity.current
    val dpi = (density.density * BASE_DPI).toInt()
    val images =
        remember(symbols, density.density) {
            symbols.associate { it.id to SymbolBitmaps.draw(it, density.density, dpi).asImageBitmap() }
        }
    var group: String? = null
    entries.forEach { e ->
        if (e.group != group) {
            group = e.group
            Text(e.group, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(top = 4.dp))
        }
        Row(Modifier.fillMaxWidth().heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(72.dp), contentAlignment = Alignment.Center) {
                e.lineColor?.let { LineSample(Color(it.toColorInt()), e.lineDashed) }
                e.symbol?.let { id -> images[id]?.let { Image(it, contentDescription = null) } }
            }
            Text(e.label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 8.dp))
        }
    }
}

@Composable
private fun LineSample(
    color: Color,
    dashed: Boolean,
) {
    Canvas(Modifier.size(width = 64.dp, height = 20.dp)) {
        val y = size.height / 2
        val effect = if (dashed) PathEffect.dashPathEffect(floatArrayOf(DASH_ON, DASH_OFF)) else null
        drawLine(Color.White, Offset(0f, y), Offset(size.width, y), strokeWidth = 14f, cap = StrokeCap.Round)
        drawLine(color, Offset(0f, y), Offset(size.width, y), strokeWidth = 8f, cap = StrokeCap.Round, pathEffect = effect)
    }
}

private const val BASE_DPI = 160f
private const val DASH_ON = 14f
private const val DASH_OFF = 10f
