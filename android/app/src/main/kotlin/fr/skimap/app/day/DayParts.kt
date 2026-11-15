package fr.skimap.app.day

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import fr.skimap.app.R
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.DayStart
import fr.skimap.core.GroupSpec
import fr.skimap.core.GroupView
import fr.skimap.core.LoopView
import fr.skimap.core.MorningView
import java.time.format.DateTimeFormatter
import java.util.Locale

private val DURATIONS = listOf(60, 90, 120, 180, 240)
private val RETURNS = listOf(null, "15:00", "16:00", "17:00")

private const val MINUTES_PER_HOUR = 60

private fun durationLabel(min: Int): String {
    val h = min / MINUTES_PER_HOUR
    val m = min % MINUTES_PER_HOUR
    return if (m == 0) "$h h" else "$h h ${"%02d".format(m)}"
}

@Composable
internal fun Section(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ChoiceRow(
    title: String,
    content: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, style = MaterialTheme.typography.labelMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) { content() }
    }
}

@Composable
internal fun Choice(
    text: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(text, style = MaterialTheme.typography.bodyMedium) },
        modifier = modifier.heightIn(min = 48.dp),
    )
}

@Composable
internal fun Inputs(
    state: DayUi.Ready,
    actions: DayActions,
    onNewGroup: () -> Unit,
) {
    val i = state.inputs
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        ChoiceRow(stringResource(R.string.day_start)) {
            state.starts.forEach { s: DayStart ->
                Choice(s.name.replaceFirstChar { it.titlecase(Locale.FRANCE) }, i.start == s, { actions.setInputs { it.copy(start = s) } })
            }
        }
        ChoiceRow(stringResource(R.string.day_duration)) {
            DURATIONS.forEach { d -> Choice(durationLabel(d), i.budgetMin == d, { actions.setInputs { it.copy(budgetMin = d) } }) }
        }
        ChoiceRow(stringResource(R.string.day_style)) {
            Choice(stringResource(R.string.day_classic), !i.skating, { actions.setInputs { it.copy(skating = false) } })
            Choice(stringResource(R.string.day_skating), i.skating, { actions.setInputs { it.copy(skating = true) } })
        }
        ChoiceRow(stringResource(R.string.day_return)) {
            RETURNS.forEach { r ->
                val label = r?.replace(":00", " h") ?: stringResource(R.string.day_return_free)
                Choice(label, i.returnBefore == r, { actions.setInputs { it.copy(returnBefore = r) } })
            }
        }
        ChoiceRow(stringResource(R.string.day_pistes)) {
            Choice(stringResource(R.string.day_pistes_all), !i.easyOnly, { actions.setInputs { it.copy(easyOnly = false) } })
            Choice(stringResource(R.string.day_pistes_easy), i.easyOnly, { actions.setInputs { it.copy(easyOnly = true) } })
        }
        ChoiceRow(stringResource(R.string.day_group)) {
            state.groups.forEach { g -> Choice(g.spec.name, state.group?.spec?.name == g.spec.name, { actions.selectGroup(g) }) }
            Choice("+ " + stringResource(R.string.day_group_new), false, onNewGroup, Modifier.testTag(DayTags.GROUP_NEW))
        }
        state.group?.let { g -> Text("${g.label} · ${g.learned}", style = MaterialTheme.typography.bodySmall) }
    }
}

@Composable
internal fun Card(
    title: String,
    content: @Composable () -> Unit,
) {
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(title, style = MaterialTheme.typography.labelMedium)
            content()
        }
    }
}

@Composable
internal fun MorningCards(m: MorningView) {
    var all by rememberSaveable { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Card(stringResource(R.string.day_briefing)) {
            // The first line repeats the site and date shown above.
            val lines = m.briefing.drop(1)
            val shown = if (all) lines else lines.filter { it.startsWith("Coucher") || it.startsWith("Qualité") }.take(2)
            shown.forEach { Text(it, style = MaterialTheme.typography.bodySmall) }
            if (!all && lines.isNotEmpty()) {
                TextButton(onClick = { all = true }, modifier = Modifier.heightIn(min = 48.dp)) {
                    Text(stringResource(R.string.day_briefing_more))
                }
            }
        }
        m.bestTime?.let { Card(stringResource(R.string.day_best_time)) { Text(it, style = MaterialTheme.typography.bodyMedium) } }
        m.wax?.let { Card(stringResource(R.string.day_wax)) { Text(it, style = MaterialTheme.typography.bodySmall) } }
    }
}

@Composable
internal fun LoopCard(
    l: LoopView,
    selected: Boolean,
    onSelect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier =
            modifier
                .fillMaxWidth()
                .selectable(selected = selected, onClick = onSelect, role = Role.RadioButton),
        shape = RoundedCornerShape(12.dp),
        color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainer,
        contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface,
        border = if (selected) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
    ) {
        Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            RadioButton(selected = selected, onClick = null)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(l.label, style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(R.string.day_loop_stats, "%.1f".format(Locale.FRANCE, l.lengthKm), l.gainM.toInt()),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text("${l.timeText} · ${l.backText}", style = MaterialTheme.typography.bodyMedium)
                l.warning?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyMedium,
                        color = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.error,
                    )
                }
                // Open, closed, unknown.
                if (l.statusKm.size == STATUS_PARTS) {
                    Text(
                        stringResource(
                            R.string.day_loop_status,
                            "%.1f".format(Locale.FRANCE, l.statusKm[0]),
                            "%.1f".format(Locale.FRANCE, l.statusKm[1]),
                        ),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                if (selected) {
                    l.reasons.take(REASONS_SHOWN).forEach { Text("· $it", style = MaterialTheme.typography.bodySmall) }
                    if (l.equipment.isNotEmpty()) {
                        Text(
                            stringResource(R.string.day_equipment),
                            style = MaterialTheme.typography.labelMedium,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                        l.equipment.forEach { Text("· $it", style = MaterialTheme.typography.bodySmall) }
                    }
                }
            }
        }
    }
}

private const val REASONS_SHOWN = 4
private const val STATUS_PARTS = 3
