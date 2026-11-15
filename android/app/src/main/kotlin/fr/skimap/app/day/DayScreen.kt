package fr.skimap.app.day

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.foundation.lazy.LazyListScope
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

/** Test tags of the morning screen. */
object DayTags {
    const val LIST = "day_list"
    const val VALIDATE = "day_validate"
    const val LOOP = "day_loop_"
    const val GROUP_NEW = "day_group_new"
    const val CONTACT = "day_contact"
}

/** Callbacks of [DayScreen]. */
data class DayActions(
    val setInputs: ((DayInputs) -> DayInputs) -> Unit = {},
    val choose: (Int) -> Unit = {},
    val selectGroup: (GroupView) -> Unit = {},
    val addGroup: (GroupSpec) -> Unit = {},
    val setContact: (Contact?) -> Unit = {},
    val validate: () -> Unit = {},
)

/**
 * The morning (J5): everything decided here, without gloves, in one screen — start, time,
 * technique, return, pistes, group; the day's briefing, best time and wax; the loops with
 * the group's time (a range while the group has little history: C11); the contact who gets
 * the plan; one button, « Valider la journée ».
 */
@Composable
fun DayScreen(
    state: DayUi.Ready,
    actions: DayActions,
    modifier: Modifier = Modifier,
    validating: Boolean = false,
) {
    var groupDialog by rememberSaveable { mutableStateOf(false) }
    var contactDialog by rememberSaveable { mutableStateOf(false) }
    if (groupDialog) GroupDialog(onSave = actions.addGroup, onClose = { groupDialog = false })
    if (contactDialog) ContactDialog(state.contact, onSave = actions.setContact, onClose = { contactDialog = false })
    Column(modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().testTag(DayTags.LIST),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            item { DayHeader(state) }
            item { Inputs(state, actions, onNewGroup = { groupDialog = true }) }
            morningItems(state, actions)
            item { ContactCard(state.contact, onClick = { contactDialog = true }) }
            state.morning?.let { m -> item { Notes(m) } }
        }
        HorizontalDivider()
        Button(
            onClick = actions.validate,
            enabled = !validating && !state.computing && !state.morning?.loops.isNullOrEmpty(),
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(12.dp)
                    .heightIn(min = GloveTarget)
                    .testTag(DayTags.VALIDATE),
        ) {
            Text(
                stringResource(if (validating) R.string.day_validating else R.string.day_validate),
                style = MaterialTheme.typography.labelLarge,
            )
        }
    }
}

/** The loops of the morning: the day's cards, then the loops to choose from. */
private fun LazyListScope.morningItems(
    state: DayUi.Ready,
    actions: DayActions,
) {
    val m = state.morning
    if (m != null) item { MorningCards(m) }
    item {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Section(stringResource(R.string.day_loops))
            if (state.computing) CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 3.dp)
        }
    }
    if (m != null && m.loops.isEmpty() && !state.computing) {
        item { Text(stringResource(R.string.day_no_loop), style = MaterialTheme.typography.bodyMedium) }
    }
    itemsIndexed(m?.loops.orEmpty(), key = { i, l -> "$i ${l.label}" }) { i, l ->
        LoopCard(l, selected = i == state.choice, onSelect = { actions.choose(i) }, modifier = Modifier.testTag(DayTags.LOOP + i))
    }
    state.error?.let { e -> item { Text(e, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium) } }
}

/** Loading or no pack to plan on. */
@Composable
fun DayMessage(
    text: String?,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        if (text == null) CircularProgressIndicator() else Text(text, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
internal fun DayHeader(state: DayUi.Ready) {
    Column {
        Text(
            stringResource(R.string.day_title, state.siteName),
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.semantics { heading() },
        )
        Text(
            state.inputs.date
                .format(DateTimeFormatter.ofPattern("EEEE d MMMM", Locale.FRANCE))
                .replaceFirstChar { it.titlecase(Locale.FRANCE) },
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

/** The contact who gets the plan (K10); touch to change. */
@Composable
internal fun ContactCard(
    contact: Contact?,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp).testTag(DayTags.CONTACT),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(Modifier.padding(12.dp)) {
            Text(stringResource(R.string.day_contact), style = MaterialTheme.typography.labelMedium)
            Text(
                contact?.let { "${it.name} · ${it.number}" } ?: stringResource(R.string.day_contact_none),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

/** Missing data and « never a guarantee ». */
@Composable
internal fun Notes(m: MorningView) {
    Column {
        if (m.notes.isNotEmpty()) {
            Text(stringResource(R.string.day_notes), style = MaterialTheme.typography.labelMedium)
            m.notes.forEach { Text("· $it", style = MaterialTheme.typography.bodySmall) }
        }
        Text(m.neverSafe, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 8.dp))
    }
}
