package fr.skimap.app.offpiste

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.lifecycleScope
import fr.skimap.app.R
import fr.skimap.app.data.PlatformLocator
import fr.skimap.app.fog.FogActivity
import fr.skimap.app.fog.GuidedTrackingService
import fr.skimap.app.fog.Outings
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.core.FogTarget
import fr.skimap.core.OffpistePlanner
import fr.skimap.core.OffpisteRouteView
import fr.skimap.core.fogTargets
import fr.skimap.core.openOffpiste
import fr.skimap.tracking.Tracking
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** Test tags of the off-piste screen. */
object OffpisteTags {
    const val COMPUTE = "offpiste_compute"
    const val GUIDE = "offpiste_guide"
    const val RESULT = "offpiste_result"
    const val MAP = "offpiste_show_map"
}

/** What the off-piste screen shows. */
data class OffpisteUi(
    val points: List<FogTarget> = emptyList(),
    val from: FogTarget? = null,
    val to: FogTarget? = null,
    val day: LocalDate = LocalDate.now(),
    val working: Boolean = false,
    val route: OffpisteRouteView? = null,
    val error: String? = null,
)

/** Callbacks of [OffpisteScreen]. */
data class OffpisteActions(
    val from: (FogTarget) -> Unit = {},
    val to: (FogTarget) -> Unit = {},
    val day: (Long) -> Unit = {},
    val compute: () -> Unit = {},
    val showOnMap: () -> Unit = {},
    val guide: () -> Unit = {},
)

/**
 * Off-piste SRN route (V1): from A to B off the trails with the core (slopes under 30°,
 * zones at the chosen day, avalanche exposure, snow of the pack), its difficulty both ways
 * with its reasons and advice (SR1), its time at the group's pace (C11), the line drawn
 * here and on the map, then the guidance by the same location service as the day plan.
 */
class OffpisteActivity : ComponentActivity() {
    private var ui by mutableStateOf(OffpisteUi())
    private var planner: OffpistePlanner? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val site = intent.getStringExtra(EXTRA_SITE)
        val point =
            if (intent.hasExtra(EXTRA_LAT)) {
                FogTarget(
                    "point",
                    getString(R.string.fog_kind_point),
                    intent.getDoubleExtra(EXTRA_LON, 0.0),
                    intent.getDoubleExtra(EXTRA_LAT, 0.0),
                    null,
                )
            } else {
                null
            }
        lifecycleScope.launch {
            val here = PlatformLocator(applicationContext).locate()
            val pack = withContext(Dispatchers.IO) { Outings.packDir(applicationContext, site) }
            if (pack == null) {
                ui = ui.copy(error = getString(R.string.tools_no_pack))
                return@launch
            }
            val list =
                withContext(Dispatchers.IO) {
                    runCatching { fogTargets(pack.path, here?.longitude, here?.latitude, Outings.lang(), 0u) }.getOrDefault(emptyList())
                }
            val me = here?.let { FogTarget("here", getString(R.string.offpiste_here), it.longitude, it.latitude, 0.0) }
            val points = listOfNotNull(me, point) + list
            ui = ui.copy(points = points, from = me ?: list.firstOrNull(), to = point ?: list.getOrNull(1))
            planner =
                withContext(Dispatchers.IO) {
                    runCatching { openOffpiste(pack.path, Outings.workDir(applicationContext).path) }
                        .onFailure { ui = ui.copy(error = it.message) }
                        .getOrNull()
                }
        }
        setContent {
            SkimapTheme {
                Surface {
                    OffpisteScreen(
                        ui,
                        OffpisteActions(
                            from = { ui = ui.copy(from = it, route = null) },
                            to = { ui = ui.copy(to = it, route = null) },
                            day = { ui = ui.copy(day = ui.day.plusDays(it), route = null) },
                            compute = ::compute,
                            showOnMap = {
                                ui.route?.let { RouteOverlay.show(it.line) }
                                finish()
                            },
                            guide = ::guide,
                        ),
                    )
                }
            }
        }
    }

    @Suppress("ReturnCount")
    private fun compute() {
        val p = planner ?: return
        val (a, b) = (ui.from ?: return) to (ui.to ?: return)
        ui = ui.copy(working = true, error = null)
        lifecycleScope.launch {
            // Zones at 10 h on the chosen day.
            val at =
                ui.day
                    .atTime(ROUTE_HOUR, 0)
                    .atZone(ZoneId.of("Europe/Paris"))
                    .toInstant()
                    .toEpochMilli()
            val r =
                withContext(Dispatchers.Default) {
                    runCatching { p.route(a, b, at, Outings.groupJson(applicationContext)) }
                }
            ui = ui.copy(working = false, route = r.getOrNull(), error = r.exceptionOrNull()?.message)
        }
    }

    @Suppress("ReturnCount")
    private fun guide() {
        val p = planner ?: return
        val r = ui.route ?: return
        if (!FogActivity.granted(this)) {
            Toast.makeText(this, R.string.fog_permission, Toast.LENGTH_LONG).show()
            return
        }
        lifecycleScope.launch {
            val dir =
                withContext(Dispatchers.IO) {
                    val d = Outings.newOutingDir(applicationContext)
                    runCatching {
                        p.guide(d.path, Outings.lang())
                        d.path
                    }.getOrNull()
                }
            if (dir != null) {
                RouteOverlay.show(r.line)
                Tracking.start(this@OffpisteActivity, GuidedTrackingService::class.java, dir)
                finish()
            }
        }
    }

    companion object {
        const val EXTRA_SITE = "site"
        const val EXTRA_LAT = "lat"
        const val EXTRA_LON = "lon"
        private const val ROUTE_HOUR = 10

        fun intent(
            c: Context,
            site: String?,
            lat: Double? = null,
            lon: Double? = null,
        ): Intent =
            Intent(c, OffpisteActivity::class.java).putExtra(EXTRA_SITE, site).apply {
                if (lat != null && lon != null) putExtra(EXTRA_LAT, lat).putExtra(EXTRA_LON, lon)
            }
    }
}

/** The off-piste screen, stateless (captured in the JVM tests). */
@Composable
fun OffpisteScreen(
    ui: OffpisteUi,
    actions: OffpisteActions,
) {
    var details by rememberSaveable { mutableStateOf(false) }
    LazyColumn(
        Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Text(
                stringResource(R.string.offpiste_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.semantics { heading() },
            )
        }
        item { PointChoice(stringResource(R.string.offpiste_from), ui.points, ui.from, actions.from) }
        item { PointChoice(stringResource(R.string.offpiste_to), ui.points, ui.to, actions.to) }
        item {
            Text(
                stringResource(
                    R.string.offpiste_day,
                    ui.day.format(DateTimeFormatter.ofPattern("EEEE d MMMM", LocalConfiguration.current.locales[0])),
                ),
                style = MaterialTheme.typography.bodyLarge,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = {
                    actions.day(-1)
                }, modifier = Modifier.weight(1f).heightIn(min = GloveTarget)) { Text(stringResource(R.string.offpiste_day_prev)) }
                OutlinedButton(onClick = {
                    actions.day(1)
                }, modifier = Modifier.weight(1f).heightIn(min = GloveTarget)) { Text(stringResource(R.string.offpiste_day_next)) }
            }
        }
        item {
            Button(
                onClick = actions.compute,
                enabled = !ui.working && ui.from != null && ui.to != null,
                modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(OffpisteTags.COMPUTE),
            ) { Text(stringResource(if (ui.working) R.string.tools_working else R.string.offpiste_compute)) }
        }
        ui.error?.let { e ->
            item {
                Surface(
                    color = MaterialTheme.colorScheme.errorContainer,
                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                    shape = MaterialTheme.shapes.medium,
                ) {
                    Text(e, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(12.dp))
                }
            }
        }
        val r = ui.route ?: return@LazyColumn
        item { RouteResult(r) }
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(
                    onClick = actions.showOnMap,
                    modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(OffpisteTags.MAP),
                ) {
                    Text(stringResource(R.string.offpiste_show_map))
                }
                Button(onClick = actions.guide, modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(OffpisteTags.GUIDE)) {
                    Text(stringResource(R.string.offpiste_guide))
                }
            }
        }
        item {
            OutlinedButton(onClick = { details = !details }, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
                Text(stringResource(R.string.offpiste_details))
            }
            if (details) Text(r.text, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

@Composable
private fun RouteResult(r: OffpisteRouteView) {
    Column(Modifier.testTag(OffpisteTags.RESULT), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(r.label, style = MaterialTheme.typography.titleMedium)
        Text(stringResource(R.string.offpiste_level, r.level, r.levelReverse), style = MaterialTheme.typography.titleMedium)
        Text(
            stringResource(
                R.string.offpiste_figures,
                "%.1f".format(LocalConfiguration.current.locales[0], r.lengthKm),
                r.ascentM.toInt(),
                r.descentM.toInt(),
                r.maxSlopeDeg.toInt(),
            ),
            style = MaterialTheme.typography.bodyLarge,
        )
        Text(r.timeText, style = MaterialTheme.typography.bodyLarge)
        RouteSketch(r.line, Modifier.fillMaxWidth().aspectRatio(SKETCH_RATIO))
        Text(stringResource(R.string.offpiste_reasons), style = MaterialTheme.typography.titleSmall)
        r.reasons.forEach { Text("· $it", style = MaterialTheme.typography.bodyMedium) }
        Text(stringResource(R.string.offpiste_advice), style = MaterialTheme.typography.titleSmall)
        r.advice.forEach { Text("· $it", style = MaterialTheme.typography.bodyMedium) }
        if (r.inPlaces.isNotEmpty()) {
            Text(
                stringResource(R.string.offpiste_in_places, r.inPlaces.joinToString()),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        if (r.missing.isNotEmpty()) {
            Text(
                stringResource(R.string.offpiste_missing, r.missing.joinToString()),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        Surface(
            color = MaterialTheme.colorScheme.errorContainer,
            contentColor = MaterialTheme.colorScheme.onErrorContainer,
            shape = MaterialTheme.shapes.medium,
        ) {
            Text(r.neverGuarantee, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(12.dp))
        }
        Text(stringResource(R.string.offpiste_compute_ms, r.computeMs.toInt()), style = MaterialTheme.typography.bodySmall)
    }
}

/** The line of the route, north up, fitted in the box: start dot, end ring. */
@Composable
private fun RouteSketch(
    line: List<Double>,
    modifier: Modifier = Modifier,
) {
    val color = MaterialTheme.colorScheme.primary
    val ends = MaterialTheme.colorScheme.onSurface
    Canvas(modifier) {
        if (line.size < MIN_COORDS) return@Canvas
        val lons = line.filterIndexed { i, _ -> i % 2 == 0 }
        val lats = line.filterIndexed { i, _ -> i % 2 == 1 }
        val kx = kotlin.math.cos(Math.toRadians(lats.average()))
        val w = (lons.max() - lons.min()) * kx
        val h = lats.max() - lats.min()
        val pad = SKETCH_PAD
        val s = minOf((size.width - 2 * pad) / w.coerceAtLeast(TINY), (size.height - 2 * pad) / h.coerceAtLeast(TINY))

        fun pt(i: Int) =
            Offset(
                (pad + (lons[i] - lons.min()) * kx * s).toFloat(),
                (size.height - pad - (lats[i] - lats.min()) * s).toFloat(),
            )
        val p =
            Path().apply {
                moveTo(pt(0).x, pt(0).y)
                for (i in 1 until lons.size) lineTo(pt(i).x, pt(i).y)
            }
        drawPath(p, color, style = Stroke(LINE_PX))
        drawCircle(ends, START_PX, pt(0))
        drawCircle(ends, END_PX, pt(lons.size - 1), style = Stroke(END_RING_PX))
    }
}

@Composable
private fun PointChoice(
    title: String,
    points: List<FogTarget>,
    selected: FogTarget?,
    onPick: (FogTarget) -> Unit,
) {
    var open by rememberSaveable { mutableStateOf(false) }
    Column {
        Text(title, style = MaterialTheme.typography.titleSmall)
        OutlinedButton(onClick = { open = !open }, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(selected?.name ?: "—")
        }
        if (open) {
            points.take(MAX_POINTS).forEach { t ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = GloveTarget)
                        .selectable(t == selected, role = Role.RadioButton) {
                            open = false
                            onPick(t)
                        },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = t == selected, onClick = null)
                    val d =
                        t.distanceM
                            ?.takeIf { it > 0 }
                            ?.let { " · %.1f km".format(LocalConfiguration.current.locales[0], it / M_PER_KM) }
                            .orEmpty()
                    Text(t.name + d, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
    }
}

private const val MAX_POINTS = 25
private const val MIN_COORDS = 4
private const val SKETCH_PAD = 16f
private const val TINY = 1e-9
private const val LINE_PX = 6f
private const val START_PX = 10f
private const val END_PX = 12f
private const val END_RING_PX = 4f
private const val M_PER_KM = 1000.0
private const val SKETCH_RATIO = 1.5f
