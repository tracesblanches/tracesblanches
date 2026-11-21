package fr.skimap.app.fog

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.hardware.SensorManager
import android.location.Location
import android.location.LocationManager
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.core.location.LocationListenerCompat
import androidx.core.location.LocationManagerCompat
import androidx.core.location.LocationRequestCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import fr.skimap.app.R
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.core.BearingView
import fr.skimap.core.FogTarget
import fr.skimap.core.fogBearing
import fr.skimap.core.fogPoint
import fr.skimap.core.fogTargets
import fr.skimap.core.guidedBearing
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.round

/** Test tags of the bearing screen. */
object FogTags {
    const val ARROW = "fog_arrow"
    const val VOICE = "fog_voice"
    const val CALIBRATION = "fog_calibration"

    /** Where the position comes from and how precise it is (#181). */
    const val FIX = "fog_fix"

    /** The arrow of « ramène-moi » over the map ([BackArrow]). */
    const val BACK_HINT = "back_arrow_hint"
    const val BACK_ARROW = "back_arrow"
}

/** What the bearing screen shows. */
data class FogUi(
    val targets: List<FogTarget> = emptyList(),
    val target: FogTarget? = null,
    val bearing: BearingView? = null,
    val compass: CompassReading = CompassReading(null, Calibration.NONE),
    val guiding: Boolean = false,
    /** The position the bearing is measured from, quick or GPS (#181). */
    val fix: Fix? = null,
    /** Now, ms since the epoch: the age of [fix]. */
    val nowMs: Long = 0L,
)

/**
 * Navigation on a bearing in the fog (K7): the point to reach (a refuge, a car park, a
 * start, or the point touched on the map), the bearing and the distance, an arrow turned
 * by the calibrated compass while the screen is on, and the voice guidance with the screen
 * off (the location service, [GuidedTrackingService]).
 */
class FogActivity : ComponentActivity() {
    private var ui by mutableStateOf(FogUi())
    private lateinit var compass: Compass
    private var hasCompass = false
    private var here: Fix? = null
    private val listener = LocationListenerCompat { onFix(it) }

    /** The fused or network provider, until the GPS has a fix (#181). */
    private val quickListener = LocationListenerCompat { onFix(it) }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        compass = Compass(getSystemService(SensorManager::class.java))
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
        ui = ui.copy(target = point)
        lifecycleScope.launch {
            val pack = withContext(Dispatchers.IO) { Outings.packDir(applicationContext, intent.getStringExtra(EXTRA_SITE)) }
            val list =
                pack?.let { p ->
                    withContext(
                        Dispatchers.IO,
                    ) { runCatching { fogTargets(p.path, null, null, Outings.lang(), 0u) }.getOrDefault(emptyList()) }
                } ?: emptyList()
            // The point of the map named by the nearest place of the pack (#181).
            val named =
                point?.let { pt ->
                    pack?.let { p ->
                        withContext(Dispatchers.IO) { runCatching { fogPoint(p.path, pt.lon, pt.lat, Outings.lang()) }.getOrNull() }
                    } ?: pt
                }
            val target = if (ui.target == point) named else ui.target
            ui = ui.copy(targets = listOfNotNull(named) + list, target = target ?: list.firstOrNull()).withBearing()
        }
        lifecycleScope.launch { compass.reading.collect { r -> ui = ui.copy(compass = r).withBearing() } }
        setContent {
            SkimapTheme {
                Surface {
                    val tracking by Tracking.state.collectAsStateWithLifecycle()
                    val guiding = (tracking as? TrackingState.Running)?.guidance?.route == "fog"
                    FogScreen(ui.copy(guiding = guiding), hasCompass, onPick = { ui = ui.copy(target = it).withBearing() }, onVoice = {
                        if (guiding) {
                            Tracking.command(this, GuidedTrackingService::class.java, Tracking.ACTION_STOP)
                        } else {
                            startVoice()
                        }
                    })
                }
            }
        }
    }

    private val ask =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { granted ->
            if (granted[Manifest.permission.ACCESS_FINE_LOCATION] == true) {
                listen()
            } else {
                Toast.makeText(this, R.string.fog_permission, Toast.LENGTH_LONG).show()
            }
        }

    override fun onStart() {
        super.onStart()
        hasCompass = compass.start()
        if (granted(this)) {
            listen()
        } else {
            ask.launch(
                fr.skimap.app.day.DayActivity
                    .permissionsForStart(),
            )
        }
    }

    @SuppressLint("MissingPermission")
    override fun onStop() {
        compass.stop()
        getSystemService(LocationManager::class.java)?.let {
            LocationManagerCompat.removeUpdates(it, listener)
            LocationManagerCompat.removeUpdates(it, quickListener)
        }
        super.onStop()
    }

    /**
     * Fixes every second while the screen is shown (interaction), none otherwise: the GPS,
     * and until it has a fix the last known position and the fused (else network)
     * provider, so that the bearing shows in a few seconds rather than after the first GPS
     * fix (#181).
     */
    @SuppressLint("MissingPermission")
    private fun listen() {
        val lm = getSystemService(LocationManager::class.java) ?: return
        if (!granted(this)) return
        val req = LocationRequestCompat.Builder(FIX_MS).setQuality(LocationRequestCompat.QUALITY_HIGH_ACCURACY).build()
        LocationManagerCompat.requestLocationUpdates(lm, LocationManager.GPS_PROVIDER, req, listener, mainLooper)
        // A last known position, even of the GPS, is not a fix of now: quick, with its time.
        val known =
            listOfNotNull(LocationManager.GPS_PROVIDER, quickProvider(lm), LocationManager.NETWORK_PROVIDER)
                .distinct()
                .mapNotNull { p -> runCatching { lm.getLastKnownLocation(p) }.getOrNull()?.let { Fix.of(it).copy(gps = false) } }
        QuickFix.freshest(known, System.currentTimeMillis())?.let(::onFix)
        quickProvider(lm)?.let { p ->
            val quick = LocationRequestCompat.Builder(FIX_MS).setQuality(LocationRequestCompat.QUALITY_BALANCED_POWER_ACCURACY).build()
            LocationManagerCompat.requestLocationUpdates(lm, p, quick, quickListener, mainLooper)
        }
    }

    private fun quickProvider(lm: LocationManager): String? =
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && lm.hasProvider(LocationManager.FUSED_PROVIDER) -> {
                LocationManager.FUSED_PROVIDER
            }

            lm.isProviderEnabled(LocationManager.NETWORK_PROVIDER) -> {
                LocationManager.NETWORK_PROVIDER
            }

            else -> {
                null
            }
        }

    private fun onFix(l: Location) = onFix(Fix.of(l))

    @SuppressLint("MissingPermission") // removing updates, as in onStop
    private fun onFix(f: Fix) {
        val now = System.currentTimeMillis()
        if (!QuickFix.replaces(here, f, now)) return
        here = f
        if (f.gps) {
            // The GPS has its fix: the quick provider is no longer needed.
            getSystemService(LocationManager::class.java)?.let { LocationManagerCompat.removeUpdates(it, quickListener) }
        }
        compass.setPlace(f.lat, f.lon, f.altitudeM, f.timeMs)
        ui = ui.copy(fix = f, nowMs = now).withBearing()
    }

    @Suppress("ReturnCount")
    private fun FogUi.withBearing(): FogUi {
        val h = here ?: return copy(bearing = null)
        val t = target ?: return copy(bearing = null)
        val heading = compass.headingDeg ?: h.courseDeg
        return copy(bearing = fogBearing(h.lon, h.lat, t.lon, t.lat, heading, Outings.lang()))
    }

    private fun startVoice() {
        val t = ui.target ?: return
        if (!granted(this)) {
            ask.launch(
                fr.skimap.app.day.DayActivity
                    .permissionsForStart(),
            )
            return
        }
        lifecycleScope.launch {
            val dir =
                withContext(Dispatchers.IO) {
                    val d = Outings.newOutingDir(applicationContext)
                    val pack = Outings.packDir(applicationContext, intent.getStringExtra(EXTRA_SITE))
                    runCatching {
                        guidedBearing(d.path, t, Outings.groupJson(applicationContext), 0.0, Outings.lang(), pack?.name, pack?.path)
                        d.path
                    }.getOrNull()
                }
            if (dir != null) Tracking.start(this@FogActivity, GuidedTrackingService::class.java, dir)
        }
    }

    companion object {
        const val EXTRA_SITE = "site"
        const val EXTRA_LAT = "lat"
        const val EXTRA_LON = "lon"
        private const val FIX_MS = 1000L

        fun granted(c: Context) =
            ContextCompat.checkSelfPermission(c, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED

        fun intent(
            c: Context,
            site: String?,
            lat: Double? = null,
            lon: Double? = null,
        ): Intent =
            Intent(c, FogActivity::class.java).putExtra(EXTRA_SITE, site).apply {
                if (lat != null && lon != null) putExtra(EXTRA_LAT, lat).putExtra(EXTRA_LON, lon)
            }
    }
}

/** The bearing screen, stateless (captured in the JVM tests). */
@Composable
fun FogScreen(
    ui: FogUi,
    hasCompass: Boolean,
    onPick: (FogTarget) -> Unit,
    onVoice: () -> Unit,
) {
    LazyColumn(
        Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Text(
                stringResource(R.string.fog_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.semantics { heading() },
            )
        }
        item { BearingPanel(ui, hasCompass) }
        item {
            val label = stringResource(if (ui.guiding) R.string.fog_voice_stop else R.string.fog_voice)
            Button(
                onClick = onVoice,
                enabled = ui.target != null,
                modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(FogTags.VOICE),
            ) {
                Text(label)
            }
        }
        item {
            Text(
                stringResource(R.string.fog_choose),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.semantics { heading() },
            )
        }
        items(ui.targets, key = { "${it.kind}:${it.name}:${it.lon}" }) { t ->
            TargetRow(t, ui.fix, selected = t == ui.target) { onPick(t) }
        }
    }
}

@Composable
private fun BearingPanel(
    ui: FogUi,
    hasCompass: Boolean,
) {
    val b = ui.bearing
    val target = ui.target
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        target?.let { Text(it.name, style = MaterialTheme.typography.titleMedium) }
        if (b == null) {
            Text(stringResource(R.string.fog_waiting_fix), style = MaterialTheme.typography.bodyLarge)
        } else {
            val rel = b.relativeDeg
            val description = stringResource(R.string.fog_arrow, target?.name.orEmpty()) + (b.turnText?.let { ", $it" } ?: "")
            BearingArrow(rel ?: 0.0, Modifier.size(160.dp).testTag(FogTags.ARROW).semantics { contentDescription = description })
            Text(b.distanceText, style = MaterialTheme.typography.headlineMedium)
            Text(b.bearingText, style = MaterialTheme.typography.titleMedium)
            b.turnText?.let { Text(it, style = MaterialTheme.typography.titleMedium) }
        }
        ui.fix?.let { FixLine(it, ui.nowMs) }
        val cal =
            when {
                !hasCompass -> R.string.fog_compass_none
                ui.compass.calibration == Calibration.OK -> R.string.fog_compass_ok
                else -> R.string.fog_compass_low
            }
        val warn = hasCompass && ui.compass.calibration != Calibration.OK
        Surface(
            color = if (warn) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceContainer,
            contentColor = if (warn) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSurface,
            shape = MaterialTheme.shapes.medium,
            modifier = Modifier.testTag(FogTags.CALIBRATION),
        ) {
            Text(stringResource(cal), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(12.dp))
        }
    }
}

/**
 * An arrow turned by [relDeg] (0: straight ahead, positive: to the right): the bearing
 * screen's, and the arrow of « ramène-moi » on the map ([BackArrow]).
 */
@Composable
internal fun BearingArrow(
    relDeg: Double,
    modifier: Modifier = Modifier,
) {
    val color = MaterialTheme.colorScheme.primary
    val ring = MaterialTheme.colorScheme.outline
    Canvas(modifier) {
        val c = Offset(size.width / 2, size.height / 2)
        val r = size.minDimension / 2
        drawCircle(
            ring,
            radius = r,
            center = c,
            style =
                androidx.compose.ui.graphics.drawscope
                    .Stroke(RING_PX),
        )
        rotate(relDeg.toFloat(), c) {
            val p =
                Path().apply {
                    moveTo(c.x, c.y - r * ARROW_TIP)
                    lineTo(c.x + r * ARROW_HALF, c.y + r * ARROW_BACK)
                    lineTo(c.x, c.y + r * ARROW_NOTCH)
                    lineTo(c.x - r * ARROW_HALF, c.y + r * ARROW_BACK)
                    close()
                }
            drawPath(p, color)
        }
    }
}

/**
 * Where the position comes from and how precise it is (#181): « Position GPS ± 5 m », or
 * « Position rapide ± 800 m, en attente du GPS » until the GPS has a fix; its age beyond a
 * minute.
 */
@Composable
private fun FixLine(
    f: Fix,
    nowMs: Long,
) {
    val precise =
        when (val a = f.accuracyM) {
            null -> stringResource(if (f.gps) R.string.fog_fix_gps_plain else R.string.fog_fix_quick_plain)
            else -> stringResource(if (f.gps) R.string.fog_fix_gps else R.string.fog_fix_quick, accuracyText(a))
        }
    val ageMin = (nowMs - f.timeMs) / MINUTE_MS
    val text = if (ageMin >= 1) stringResource(R.string.fog_fix_age, precise, ageMin.toInt()) else precise
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = if (f.gps) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.tertiary,
        modifier = Modifier.testTag(FogTags.FIX),
    )
}

@Composable
private fun TargetRow(
    t: FogTarget,
    from: Fix?,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val kind =
        stringResource(
            when (t.kind) {
                "refuge" -> R.string.fog_kind_refuge
                "parking" -> R.string.fog_kind_parking
                "start" -> R.string.fog_kind_start
                else -> R.string.fog_kind_point
            },
        )
    // From here: distance and direction, which tell apart two points of one name (#181).
    val away =
        from?.let { f ->
            val b = remember(t.lon, t.lat, f.lon, f.lat) { fogBearing(f.lon, f.lat, t.lon, t.lat, null, Outings.lang()) }
            "${b.distanceText} · ${b.cardinal}"
        }
    // The kind once: « Parking à 300 mètres au sud du Plan », not « … · Parking ».
    val detail = listOfNotNull(kind.takeUnless { t.name.startsWith(it, ignoreCase = true) }, away).joinToString(" · ")
    Row(
        Modifier.fillMaxWidth().heightIn(min = GloveTarget).selectable(selected, role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null)
        Column(Modifier.padding(start = 8.dp)) {
            Text(t.name, style = MaterialTheme.typography.bodyLarge)
            if (detail.isNotEmpty()) {
                Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

/** « 4 m », « 850 m », « 1.2 km »: the accuracy of a position, to the metre below 1 km. */
internal fun accuracyText(
    m: Float,
    locale: java.util.Locale = java.util.Locale.getDefault(),
): String {
    val metres = maxOf(1, round(m).toInt())
    return if (m >= KM_M) distanceText(m.toDouble(), locale) else String.format(locale, "%d m", metres)
}

private const val MINUTE_MS = 60_000L
private const val KM_M = 1000f

private const val ARROW_TIP = 0.85f
private const val RING_PX = 4f
private const val ARROW_HALF = 0.45f
private const val ARROW_BACK = 0.6f
private const val ARROW_NOTCH = 0.3f
