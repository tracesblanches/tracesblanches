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

/** Children are under 18 (the priors of C11 go by age up to 17). */
private const val MAX_CHILD_AGE = 17
private const val MAX_ADULTS = 20

/** The group typed in the dialog: « 6, 9 » or « 6 9 » for the children (0 to 17 years), 1 to 20 adults. */
fun groupSpec(
    name: String,
    adults: String,
    children: String,
    level: String,
): GroupSpec {
    val ages =
        children.split(',', ' ', ';').mapNotNull {
            it
                .trim()
                .toIntOrNull()
                ?.coerceIn(0, MAX_CHILD_AGE)
                ?.toByte()
        }
    return GroupSpec(name.trim(), (adults.toIntOrNull() ?: 1).coerceIn(1, MAX_ADULTS).toUByte(), ages.toByteArray(), false, level)
}

@Composable
internal fun GroupDialog(
    onSave: (GroupSpec) -> Unit,
    onClose: () -> Unit,
) {
    var name by remember { mutableStateOf("") }
    var adults by remember { mutableStateOf("1") }
    var children by remember { mutableStateOf("") }
    var level by remember { mutableStateOf("intermediate") }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(stringResource(R.string.day_group_new)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(name, { name = it }, label = { Text(stringResource(R.string.day_group_name)) }, singleLine = true)
                OutlinedTextField(
                    adults,
                    { adults = it.filter(Char::isDigit).take(2) },
                    label = { Text(stringResource(R.string.day_group_adults)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                )
                OutlinedTextField(
                    children,
                    { children = it },
                    label = { Text(stringResource(R.string.day_group_children)) },
                    singleLine = true,
                )
                Text(stringResource(R.string.day_group_level), style = MaterialTheme.typography.labelMedium)
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    listOf(
                        "beginner" to R.string.day_level_beginner,
                        "intermediate" to R.string.day_level_intermediate,
                        "expert" to R.string.day_level_expert,
                    ).forEach { (k, r) -> Choice(stringResource(r), level == k, { level = k }) }
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = name.isNotBlank(),
                onClick = {
                    onSave(groupSpec(name, adults, children, level))
                    onClose()
                },
            ) { Text(stringResource(R.string.day_save)) }
        },
        dismissButton = { TextButton(onClick = onClose) { Text(stringResource(R.string.day_cancel)) } },
    )
}

@Composable
internal fun ContactDialog(
    contact: Contact?,
    onSave: (Contact?) -> Unit,
    onClose: () -> Unit,
) {
    var name by remember { mutableStateOf(contact?.name.orEmpty()) }
    var number by remember { mutableStateOf(contact?.number.orEmpty()) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(stringResource(R.string.day_contact)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(name, { name = it }, label = { Text(stringResource(R.string.day_contact_name)) }, singleLine = true)
                OutlinedTextField(
                    number,
                    { number = it.filter { c -> c.isDigit() || c == '+' || c == ' ' } },
                    label = { Text(stringResource(R.string.day_contact_number)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onSave(if (number.isBlank()) null else Contact(name.trim(), number.trim()))
                onClose()
            }) { Text(stringResource(R.string.day_save)) }
        },
        dismissButton = { TextButton(onClick = onClose) { Text(stringResource(R.string.day_cancel)) } },
    )
}
