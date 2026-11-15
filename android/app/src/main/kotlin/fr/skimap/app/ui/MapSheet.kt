package fr.skimap.app.ui

import androidx.compose.foundation.Canvas
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.graphics.toColorInt
import fr.skimap.app.R
import fr.skimap.app.community.CommunityEntries
import fr.skimap.app.data.Appearance
import fr.skimap.app.data.Practice
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.LayerGroup
import fr.skimap.core.LegendEntry
import fr.skimap.core.LegendKind
import java.time.LocalDate
import java.time.ZoneId

/** The primary action: area, practice, layers, legend and updates, in a bottom sheet. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MapSheet(
    state: MapUiState.Ready,
    today: LocalDate,
    zone: ZoneId,
    actions: MapActions,
    onDismiss: () -> Unit,
    autoUpdate: Boolean = true,
    appearance: Appearance? = null,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        modifier = Modifier.testTag(Tags.SHEET),
        // The handle is a touch target too: 64 dp wide instead of 32 (accessibility check).
        dragHandle = { BottomSheetDefaults.DragHandle(width = 64.dp) },
    ) {
        MapSheetContent(state, actions, onDismiss, today, zone, autoUpdate = autoUpdate, appearance = appearance)
    }
}

/** Content of [MapSheet], separate so that it can be captured and tested alone. */
@Composable
fun MapSheetContent(
    state: MapUiState.Ready,
    actions: MapActions,
    onClose: () -> Unit,
    today: LocalDate,
    zone: ZoneId,
    modifier: Modifier = Modifier,
    autoUpdate: Boolean = true,
    /** The user's light or dark choice; `null`: not offered. */
    appearance: Appearance? = null,
) {
    val texts = PackTexts(LocalResources.current)
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
        Text(texts.statusText(info.status, zone), style = MaterialTheme.typography.bodyLarge)
        Text(texts.validityText(info, today), style = MaterialTheme.typography.bodyMedium)
        SectionTitle(stringResource(R.string.sheet_site))
        Column(Modifier.selectableGroup()) {
            ChoiceRow(stringResource(R.string.mode_free), selected = state.free) { actions.selectFree() }
            if (!state.free) ChoiceRow(stringResource(R.string.mode_area, info.siteName), selected = true) {}
        }
        OutlinedButton(onClick = actions.chooseDomain, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.choose_domain), style = MaterialTheme.typography.labelLarge)
        }
        SectionTitle(stringResource(R.string.sheet_practice))
        Column(Modifier.selectableGroup()) {
            ChoiceRow(stringResource(R.string.practice_fond), selected = state.choice.practice == Practice.FOND) {
                actions.selectPractice(Practice.FOND)
            }
            ChoiceRow(stringResource(R.string.practice_srn), selected = state.choice.practice == Practice.SRN) {
                actions.selectPractice(Practice.SRN)
            }
        }
        if (state.choice.guiding) Text(stringResource(R.string.practice_guiding), style = MaterialTheme.typography.bodyMedium)
        val switches = style.groups.filter { it.toggleable }
        if (switches.isNotEmpty()) {
            SectionTitle(stringResource(R.string.sheet_layers))
            switches.forEach { g ->
                SwitchRow(groupTitle(g), checked = g.id !in state.hidden) { actions.setLayerVisible(g.id, it) }
            }
            // Safety groups cannot be hidden: a closed piste or an avalanche zone stays.
            Text(stringResource(R.string.layers_always), style = MaterialTheme.typography.bodyMedium)
        }
        if (style.legend.isNotEmpty()) {
            SectionTitle(stringResource(R.string.sheet_legend))
            Legend(style.legend)
        }
        if (appearance != null) {
            SectionTitle(stringResource(R.string.sheet_appearance))
            Column(Modifier.selectableGroup()) {
                listOf(
                    Appearance.AUTO to R.string.appearance_auto,
                    Appearance.LIGHT to R.string.appearance_light,
                    Appearance.DARK to R.string.appearance_dark,
                ).forEach { (a, label) ->
                    ChoiceRow(stringResource(label), selected = appearance == a) { actions.setAppearance(a) }
                }
            }
            // A suggestion, never forced: light reads best on snow in full sun.
            Text(stringResource(R.string.appearance_sun_hint), style = MaterialTheme.typography.bodyMedium)
        }
        actions.openSport?.let { open ->
            SectionTitle(stringResource(R.string.sheet_sport))
            OutlinedButton(onClick = open, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
                Text(stringResource(R.string.sheet_sport_open), style = MaterialTheme.typography.labelLarge)
            }
        }
        // On the ground (V8 to V15, module :community): report, « Mes sorties », fix OSM.
        CommunityEntries(info.siteId)
        SectionTitle(stringResource(R.string.sheet_updates))
        SwitchRow(stringResource(R.string.auto_update), checked = autoUpdate) { actions.setAutoUpdate(it) }
        Button(onClick = onClose, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.close), style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** Title of a layer group in the language of the app (the style's French title otherwise). */
@Composable
fun groupTitle(g: LayerGroup): String =
    when (g.id) {
        "fond" -> stringResource(R.string.group_fond)
        "pistes" -> stringResource(R.string.group_pistes)
        "dangers" -> stringResource(R.string.group_dangers)
        "reglementation" -> stringResource(R.string.group_reglementation)
        "itineraire" -> stringResource(R.string.group_itineraire)
        "position" -> stringResource(R.string.group_position)
        "pentes" -> stringResource(R.string.group_pentes)
        "refuges" -> stringResource(R.string.group_refuges)
        "points-eau" -> stringResource(R.string.group_points_eau)
        "acces" -> stringResource(R.string.group_acces)
        "noms" -> stringResource(R.string.group_noms)
        "reseau" -> stringResource(R.string.group_reseau)
        "neige" -> stringResource(R.string.group_neige)
        else -> g.title
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

/** Words of a legend row (colour is never alone). */
@Composable
private fun legendLabel(e: LegendEntry): String =
    when (e.key) {
        "novice" -> stringResource(R.string.legend_novice)
        "easy" -> stringResource(R.string.legend_easy)
        "intermediate" -> stringResource(R.string.legend_intermediate)
        "advanced" -> stringResource(R.string.legend_advanced)
        "closed" -> stringResource(R.string.legend_closed)
        "backcountry" -> stringResource(R.string.legend_backcountry)
        "hike" -> stringResource(R.string.legend_hike)
        "slope-1" -> stringResource(R.string.legend_slope_1)
        "slope-2" -> stringResource(R.string.legend_slope_2)
        "slope-3" -> stringResource(R.string.legend_slope_3)
        "slope-4" -> stringResource(R.string.legend_slope_4)
        "exposure-2" -> stringResource(R.string.legend_exposure_2)
        "exposure-3" -> stringResource(R.string.legend_exposure_3)
        else -> stringResource(R.string.legend_unknown)
    }

@Composable
private fun legendGroup(group: String): String =
    when (group) {
        "pentes" -> stringResource(R.string.legend_group_pentes)
        "dangers" -> stringResource(R.string.legend_group_dangers)
        else -> stringResource(R.string.legend_group_pistes)
    }

/** Legend of the style shown: its own colours, a sample and words for each row. */
@Composable
private fun Legend(entries: List<LegendEntry>) {
    var group: String? = null
    entries.forEach { e ->
        if (e.group != group) {
            group = e.group
            Text(legendGroup(e.group), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(top = 4.dp))
        }
        Row(Modifier.fillMaxWidth().heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(72.dp), contentAlignment = Alignment.Center) {
                Sample(Color(e.color.toColorInt()), e.kind)
            }
            Text(legendLabel(e), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 8.dp))
        }
    }
}

@Composable
private fun Sample(
    color: Color,
    kind: LegendKind,
) {
    val casing = MaterialTheme.colorScheme.surface
    val outline = MaterialTheme.colorScheme.outlineVariant
    Canvas(Modifier.size(width = 64.dp, height = 20.dp)) {
        if (kind == LegendKind.AREA) {
            drawRect(color.copy(alpha = AREA_ALPHA))
            drawRect(
                outline,
                style =
                    androidx.compose.ui.graphics.drawscope
                        .Stroke(1.dp.toPx()),
            )
            return@Canvas
        }
        val y = size.height / 2
        val effect =
            when (kind) {
                LegendKind.DASHED -> PathEffect.dashPathEffect(floatArrayOf(DASH_ON, DASH_OFF))
                LegendKind.DOTTED -> PathEffect.dashPathEffect(floatArrayOf(DOT_ON, DOT_OFF))
                else -> null
            }
        drawLine(casing, Offset(0f, y), Offset(size.width, y), strokeWidth = 14f, cap = StrokeCap.Round)
        drawLine(color, Offset(0f, y), Offset(size.width, y), strokeWidth = 8f, cap = StrokeCap.Round, pathEffect = effect)
    }
}

private const val DASH_ON = 14f
private const val DASH_OFF = 10f
private const val DOT_ON = 4f
private const val DOT_OFF = 8f
private const val AREA_ALPHA = 0.7f
