package fr.skimap.group

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/** Test tags of the group screen. */
object GroupTags {
    const val CREATE = "group_create"
    const val JOIN = "group_join"
    const val NAME = "group_name"
    const val ADD = "group_add"
    const val DONE = "group_done"
    const val QR = "group_qr"
    const val QR_FOR = "group_qr_for"
    const val MEMBER = "group_member"
    const val LEAVE = "group_leave"
    const val SCANNER = "group_scanner"
}

/** Callbacks of [GroupScreen]. */
data class GroupActions(
    val create: (List<String>) -> Unit = {},
    val scanned: (String) -> Unit = {},
    val leave: () -> Unit = {},
    val back: () -> Unit = {},
    /** Asks for the camera; the scanner opens once given. */
    val wantCamera: (() -> Unit) -> Unit = { it() },
)

/**
 * The group screen (N2): create a group (names, the creator first), show its QR code to
 * the others, or scan one and say who one is; the members with the age and the source of
 * their last position during an outing. [scanner] is the camera (a stand-in in the tests).
 */
@Composable
fun GroupScreen(
    ui: GroupUi,
    live: GroupLive?,
    hasPlan: Boolean,
    actions: GroupActions,
    modifier: Modifier = Modifier,
    scanner: @Composable (onText: (String) -> Unit, onCancel: () -> Unit) -> Unit = { onText, onCancel ->
        QrScanner(onText = onText, onCancel = onCancel)
    },
) {
    var scanning by remember { mutableStateOf(false) }
    var creating by remember { mutableStateOf(false) }
    if (scanning) {
        Box(modifier.fillMaxSize().testTag(GroupTags.SCANNER)) {
            scanner({ text ->
                scanning = false
                actions.scanned(text)
            }, { scanning = false })
        }
        return
    }
    Column(
        modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = actions.back) { Text(stringResource(R.string.group_back)) }
            Text(stringResource(R.string.group_title), style = MaterialTheme.typography.headlineSmall)
        }
        when (ui) {
            GroupUi.Loading -> {
                Unit
            }

            is GroupUi.NoGroup -> {
                if (creating) {
                    CreateForm(hasPlan, onCreate = {
                        creating = false
                        actions.create(it)
                    }, onCancel = { creating = false })
                } else {
                    NoGroup(ui, onCreate = { creating = true }, onJoin = { actions.wantCamera { scanning = true } })
                }
            }

            is GroupUi.InGroup -> {
                InGroup(ui, live, actions.leave)
            }
        }
    }
}

@Composable
private fun NoGroup(
    ui: GroupUi.NoGroup,
    onCreate: () -> Unit,
    onJoin: () -> Unit,
) {
    Text(stringResource(R.string.group_intro), style = MaterialTheme.typography.bodyMedium)
    if (ui.invalidQr) {
        Text(stringResource(R.string.group_invalid_qr), color = MaterialTheme.colorScheme.error)
    }
    Button(onClick = onCreate, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp).testTag(GroupTags.CREATE)) {
        Text(stringResource(R.string.group_create))
    }
    OutlinedButton(onClick = onJoin, modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp).testTag(GroupTags.JOIN)) {
        Text(stringResource(R.string.group_join))
    }
}

@Composable
private fun CreateForm(
    hasPlan: Boolean,
    onCreate: (List<String>) -> Unit,
    onCancel: () -> Unit,
) {
    val names = remember { mutableStateListOf("", "") }
    // A name left empty is « Membre n »: nothing to fill in but who is who.
    val defaults = (1..MAX_MEMBERS).map { stringResource(R.string.group_member_name, it) }
    names.forEachIndexed { i, n ->
        OutlinedTextField(
            value = n,
            onValueChange = { v -> names[i] = v.take(MAX_NAME) },
            label = { Text(if (i == 0) stringResource(R.string.group_your_name) else defaults[i]) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().testTag("${GroupTags.NAME}_$i"),
        )
    }
    if (names.size < MAX_MEMBERS) {
        TextButton(onClick = { names += "" }, modifier = Modifier.testTag(GroupTags.ADD)) {
            Text(stringResource(R.string.group_add_member))
        }
    }
    Text(
        stringResource(if (hasPlan) R.string.group_plan_common else R.string.group_plan_none),
        style = MaterialTheme.typography.bodySmall,
    )
    Button(
        onClick = { onCreate(names.mapIndexed { i, n -> n.ifBlank { defaults[i] } }) },
        modifier = Modifier.fillMaxWidth().heightIn(min = 56.dp).testTag(GroupTags.DONE),
    ) { Text(stringResource(R.string.group_create_done)) }
    TextButton(onClick = onCancel) { Text(stringResource(R.string.group_cancel)) }
}

@Composable
private fun InGroup(
    ui: GroupUi.InGroup,
    live: GroupLive?,
    onLeave: () -> Unit,
) {
    val id = ui.info.shortId
    val names = ui.info.members
    Text(stringResource(R.string.group_code, id), style = MaterialTheme.typography.titleMedium)
    // Each member scans their own code (it says who they are): the first other member's first.
    var shown by remember(ui.info.qrText, ui.me) { mutableIntStateOf(if (ui.me == 0 && names.size > 1) 1 else 0) }
    if (names.size > 1) {
        Text(stringResource(R.string.group_show_qr), style = MaterialTheme.typography.bodyMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            names.forEachIndexed { i, n ->
                if (i != ui.me) {
                    FilterChip(
                        selected = i == shown,
                        onClick = { shown = i },
                        label = { Text(n) },
                        modifier = Modifier.heightIn(min = 48.dp).testTag("${GroupTags.QR_FOR}_$i"),
                    )
                }
            }
        }
    }
    val qrDescription = stringResource(R.string.group_qr_description, names[shown])
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        QrCode(
            ui.qrs[shown],
            Modifier
                .widthIn(max = 320.dp)
                .fillMaxWidth()
                .testTag(GroupTags.QR)
                .semantics { contentDescription = qrDescription },
        )
    }
    Text(
        stringResource(if (ui.info.hasPlan) R.string.group_plan_common else R.string.group_plan_none),
        style = MaterialTheme.typography.bodySmall,
    )
    Text(stringResource(R.string.group_members), style = MaterialTheme.typography.titleMedium)
    when {
        live == null -> Text(stringResource(R.string.group_outside_outing), style = MaterialTheme.typography.bodySmall)
        !live.radio -> Text(stringResource(R.string.group_radio_off), color = MaterialTheme.colorScheme.error)
    }
    val members =
        live?.members ?: ui.info.members.mapIndexed { i, n ->
            Member(i, n, i == ui.me, null, null, null, null, stopped = false, offRoute = false, batteryLow = false, distanceM = null)
        }
    members.forEach { m ->
        MemberRow(m)
        HorizontalDivider()
    }
    OutlinedButton(onClick = onLeave, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag(GroupTags.LEAVE)) {
        Text(stringResource(R.string.group_leave))
    }
}

@Composable
private fun MemberRow(m: Member) {
    val status = memberStatus(m)
    Column(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .padding(vertical = 4.dp)
            .testTag("${GroupTags.MEMBER}_${m.number}")
            .semantics(mergeDescendants = true) {},
    ) {
        Text(
            if (m.me) stringResource(R.string.group_you, m.name) else m.name,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.SemiBold,
        )
        if (!m.me) Text(status, style = MaterialTheme.typography.bodyMedium)
    }
}

/** « à 450 m · position d'il y a 6 min · relayée 1 fois · arrêt prolongé ». */
@Composable
fun memberStatus(m: Member): String {
    val r = LocalResources.current
    val age = m.ageS ?: return r.getString(R.string.group_never_heard)
    val parts = ArrayList<String>()
    m.distanceM?.let { d ->
        parts +=
            if (d >= KM) {
                r.getString(R.string.group_distance_km, String.format(LocalLocale.current.platformLocale, "%.1f", d / KM))
            } else {
                r.getString(R.string.group_distance_m, (d / TEN).roundToInt() * TEN.toInt())
            }
    }
    parts +=
        when {
            age < MINUTE -> r.getString(R.string.group_just_now)
            age < HOUR -> r.getString(R.string.group_minutes_ago, (age / MINUTE).toInt())
            else -> r.getString(R.string.group_hours_ago, (age / HOUR).toInt(), ((age % HOUR) / MINUTE).toInt())
        }
    val hops = m.hops ?: 0
    parts += if (hops == 0) r.getString(R.string.group_direct) else r.getString(R.string.group_relayed, hops)
    if (m.stopped) parts += r.getString(R.string.group_stopped)
    if (m.offRoute) parts += r.getString(R.string.group_off_route)
    if (m.batteryLow) parts += r.getString(R.string.group_battery_low)
    return parts.joinToString(" · ")
}

/** The QR code, dark on white whatever the theme, with its quiet zone of 4 modules. */
@Composable
fun QrCode(
    qr: QrMatrix,
    modifier: Modifier = Modifier,
) {
    Canvas(modifier.aspectRatio(1f).background(Color.White)) {
        val n = qr.width + 2 * QUIET
        val cell = size.width / n
        for (y in 0 until qr.width) {
            for (x in 0 until qr.width) {
                if (qr.modules[y * qr.width + x]) {
                    drawRect(
                        Color.Black,
                        topLeft = Offset((x + QUIET) * cell, (y + QUIET) * cell),
                        size = Size(cell + HAIRLINE, cell + HAIRLINE),
                    )
                }
            }
        }
    }
}

private const val QUIET = 4
private const val HAIRLINE = 0.5f
private const val MAX_NAME = 24
private const val MAX_MEMBERS = 16
private const val KM = 1000.0
private const val TEN = 10.0
private const val MINUTE = 60L
private const val HOUR = 3600L
