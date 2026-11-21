package fr.skimap.app.prep

import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.work.WorkInfo
import androidx.work.WorkManager
import fr.skimap.app.R
import fr.skimap.app.appLang
import fr.skimap.app.day.DayActivity
import fr.skimap.app.day.DayStore
import fr.skimap.app.fog.Outings
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.core.BatteryView
import fr.skimap.core.InfoAge
import fr.skimap.core.PrepState
import fr.skimap.core.PreparedDay
import fr.skimap.core.prepRead
import fr.skimap.core.prepTake
import fr.skimap.core.readDayPlan
import fr.skimap.tracking.Announcer
import fr.skimap.tracking.Earcon
import fr.skimap.tracking.EarconSettings
import fr.skimap.tracking.Tracking
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.time.LocalDate
import kotlin.math.roundToInt

/** Test tags of the trip screen. */
object PrepTags {
    const val NOW = "prep_now"
    const val SKI = "prep_ski"
    const val TAKE = "prep_take"
    const val PREP = "prep_section"
    const val EARCONS = "prep_earcons"
    const val EARCON_SWITCH = "prep_earcon_switch"
    const val EARCON_VOLUME = "prep_earcon_volume"
    const val EARCON_LISTEN = "prep_earcon_listen"
}

/** What the trip screen shows. */
data class PrepUi(
    val state: PrepState = PrepState(emptyList(), emptyList()),
    val auto: Boolean = true,
    val ski: Boolean = false,
    val last: String? = null,
    val running: Boolean = false,
    val battery: BatteryView? = null,
    val lowExposure: Boolean = false,
    /** Short sounds of the guidance (K6): on, volume. */
    val earcons: EarconSettings = EarconSettings(true, EarconSettings.DEFAULT_VOLUME),
)

/** Callbacks of [PrepScreen]. */
data class PrepActions(
    val auto: (Boolean) -> Unit = {},
    val ski: (Boolean) -> Unit = {},
    val now: () -> Unit = {},
    val take: (PreparedDay) -> Unit = {},
    val lowExposure: (Boolean) -> Unit = {},
    val earcons: (EarconSettings) -> Unit = {},
    val listenEarcons: () -> Unit = {},
)

/**
 * Settings, folded (one screen of the menu), and the trip (E2): the days prepared on charge and Wi-Fi, ready to ski without
 * any heavy computation (multi-day ski mode), the date of the last update of every
 * information (never « périmé »: the skier judges), and the battery.
 */
class PrepActivity : ComponentActivity() {
    private var ui by mutableStateOf(PrepUi())

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        fr.skimap.app.rules.LowExposure
            .load(this)
        ui =
            ui.copy(
                auto = PrepWork.auto(this),
                ski = PrepWork.skiMode(this),
                lowExposure = fr.skimap.app.rules.LowExposure.shown.value,
                earcons = Tracking.earcons(this),
            )
        lifecycleScope.launch { reload() }
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                WorkManager.getInstance(applicationContext).getWorkInfosForUniqueWorkFlow("prep-now").collect { infos ->
                    val running = infos.any { it.state == WorkInfo.State.RUNNING || it.state == WorkInfo.State.ENQUEUED }
                    if (ui.running && !running) reload()
                    ui = ui.copy(running = running)
                }
            }
        }
        setContent {
            SkimapTheme {
                Surface {
                    PrepScreen(
                        ui,
                        PrepActions(
                            auto = {
                                ui = ui.copy(auto = it)
                                PrepWork.setAuto(this, it)
                            },
                            ski = {
                                ui = ui.copy(ski = it)
                                PrepWork.setSkiMode(this, it)
                            },
                            now = { PrepWork.runNow(this) },
                            take = ::take,
                            lowExposure = {
                                ui = ui.copy(lowExposure = it)
                                fr.skimap.app.rules.LowExposure
                                    .set(this, it)
                            },
                            earcons = {
                                ui = ui.copy(earcons = it)
                                Tracking.setEarcons(this, it.enabled, it.volume)
                            },
                            listenEarcons = ::listenEarcons,
                        ),
                    )
                }
            }
        }
    }

    private suspend fun reload() {
        val ctx = applicationContext
        val (state, last, battery) =
            withContext(Dispatchers.IO) {
                val s = prepRead(PrepWork.dir(ctx).path, Outings.packDirs(ctx).map { it.path }, LocalDate.now().toString(), appLang())
                val l =
                    File(PrepWork.dir(ctx), PrepWork.LAST_FILE).takeIf { it.isFile }?.let {
                        runCatching {
                            val o = JSONObject(it.readText())
                            getString(
                                R.string.prep_last_run,
                                fr.skimap.core.updatedText(o.getLong("at_ms"), appLang()),
                                o.getLong("ms"),
                                o.optString("battery_pct"),
                                o.optString("battery_c"),
                            )
                        }.getOrNull()
                    }
                Triple(s, l, Battery.forecast(ctx, null, null))
            }
        ui = ui.copy(state = state, last = last, battery = battery)
    }

    /** The turn sounds at the volume chosen: left, then right (in the headset, each on its side). */
    private fun listenEarcons() {
        val a = Announcer(applicationContext)
        a.earcon(Earcon.LEFT, ui.earcons.volume)
        a.earcon(Earcon.RIGHT, ui.earcons.volume)
        a.afterQueue { a.close() }
    }

    /** Skis a prepared plan: copied as a new outing, then started like a validated day. */
    private fun take(d: PreparedDay) {
        val from = d.planDir ?: return
        lifecycleScope.launch {
            val plan =
                withContext(Dispatchers.IO) {
                    val dir = Outings.newOutingDir(applicationContext)
                    runCatching {
                        prepTake(from, dir.path)
                        readDayPlan(dir.path, appLang())
                    }.getOrNull()
                } ?: return@launch
            DayActivity.startOuting(this@PrepActivity, plan, DayStore(this@PrepActivity).contact())
            startActivity(Intent(this@PrepActivity, DayActivity::class.java).putExtra(DayActivity.EXTRA_SITE, d.siteId))
        }
    }

    companion object {
        fun intent(c: Context): Intent = Intent(c, PrepActivity::class.java)
    }
}

/** The trip screen, stateless (captured in the JVM tests). */
@Composable
fun PrepScreen(
    ui: PrepUi,
    actions: PrepActions,
) {
    LazyColumn(
        Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Text(
                stringResource(R.string.settings_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.semantics { heading() },
            )
        }
        // What the preparation made, first when there is something; the battery only when it is a problem.
        if (ui.state.days.isNotEmpty()) {
            item {
                Text(
                    stringResource(R.string.prep_days),
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.semantics { heading() },
                )
            }
            items(ui.state.days, key = { "${it.siteId}:${it.date}" }) { d -> DayCard(d) { actions.take(d) } }
        }
        ui.battery?.takeIf { it.warning != null }?.let { b -> item { BatteryCard(b) } }
        item {
            Folded(stringResource(R.string.settings_prep), Modifier.testTag(PrepTags.PREP)) {
                SwitchLine(stringResource(R.string.prep_ski_mode), ui.ski, actions.ski, Modifier.testTag(PrepTags.SKI))
                if (ui.ski) Text(stringResource(R.string.prep_ski_mode_on), style = MaterialTheme.typography.bodyMedium)
                SwitchLine(stringResource(R.string.prep_auto), ui.auto, actions.auto)
                if (!ui.ski) {
                    Button(
                        onClick = actions.now,
                        enabled = !ui.running,
                        modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PrepTags.NOW),
                    ) { Text(stringResource(if (ui.running) R.string.tools_working else R.string.prep_now)) }
                }
                ui.last?.let { Text(stringResource(R.string.prep_last, it), style = MaterialTheme.typography.bodySmall) }
                if (ui.state.days.isEmpty()) Text(stringResource(R.string.prep_no_days), style = MaterialTheme.typography.bodyMedium)
                Text(stringResource(R.string.prep_infos), style = MaterialTheme.typography.titleSmall)
                ui.state.infos.forEach { InfoLine(it) }
            }
        }
        item { Folded(stringResource(R.string.settings_alert)) { ReturnAlertCard(null) } }
        item {
            Folded(stringResource(R.string.settings_layers)) {
                SwitchLine(stringResource(R.string.tools_low_exposure), ui.lowExposure, actions.lowExposure)
                if (ui.lowExposure) Text(stringResource(R.string.low_exposure_warning), style = MaterialTheme.typography.bodyMedium)
            }
        }
        item { Folded(stringResource(R.string.settings_earcons), Modifier.testTag(PrepTags.EARCONS)) { EarconLines(ui.earcons, actions) } }
    }
}

/** The short sounds of the guidance (K6): on or off, their volume, heard at once. */
@Composable
private fun EarconLines(
    e: EarconSettings,
    actions: PrepActions,
) {
    Text(stringResource(R.string.earcons_explain), style = MaterialTheme.typography.bodyMedium)
    SwitchLine(
        stringResource(R.string.earcons_on),
        e.enabled,
        { actions.earcons(e.copy(enabled = it)) },
        Modifier.testTag(PrepTags.EARCON_SWITCH),
    )
    if (!e.enabled) return
    Text(stringResource(R.string.earcons_volume, (e.volume * PERCENT).roundToInt()), style = MaterialTheme.typography.bodyLarge)
    Slider(
        value = e.volume,
        onValueChange = { actions.earcons(e.copy(volume = it)) },
        modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PrepTags.EARCON_VOLUME),
    )
    OutlinedButton(
        onClick = actions.listenEarcons,
        modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PrepTags.EARCON_LISTEN),
    ) { Text(stringResource(R.string.earcons_listen)) }
}

private const val PERCENT = 100

/** A section folded until touched. */
@Composable
private fun Folded(
    title: String,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    var open by rememberSaveable { mutableStateOf(false) }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = GloveTarget).toggleable(open, role = Role.Button) { open = it },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f).semantics { heading() })
            Text(if (open) "▴" else "▾", style = MaterialTheme.typography.titleMedium)
        }
        if (open) content()
    }
}

@Composable
private fun SwitchLine(
    label: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier.fillMaxWidth().heightIn(min = GloveTarget).toggleable(checked, role = Role.Switch, onValueChange = onChange),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = null)
    }
}

@Composable
private fun DayCard(
    d: PreparedDay,
    onTake: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("${d.dayText} · ${d.startName}", style = MaterialTheme.typography.titleMedium)
            Text(d.preparedText, style = MaterialTheme.typography.bodySmall)
            d.bestTime?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
            d.loops.forEach { Text("· ${it.label} — ${it.timeText}", style = MaterialTheme.typography.bodyMedium) }
            d.expectedBack?.let { Text(stringResource(R.string.prep_back, it), style = MaterialTheme.typography.bodyMedium) }
            Text(d.neverSafe, style = MaterialTheme.typography.bodySmall)
            if (d.planDir != null) {
                OutlinedButton(onClick = onTake, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PrepTags.TAKE)) {
                    Text(stringResource(R.string.prep_take))
                }
            }
        }
    }
}

@Composable
private fun InfoLine(i: InfoAge) {
    Text("${i.label} (${i.siteId}) : ${i.updatedText}", style = MaterialTheme.typography.bodyMedium)
}
