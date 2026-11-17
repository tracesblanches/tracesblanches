package fr.skimap.app.fog

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.hardware.SensorManager
import android.location.Location
import android.location.LocationManager
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
import fr.skimap.core.fogTargets
import fr.skimap.core.guidedBearing
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Test tags of the bearing screen. */
object FogTags {
    const val ARROW = "fog_arrow"
    const val VOICE = "fog_voice"
    const val CALIBRATION = "fog_calibration"

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
    private var here: Location? = null
    private val listener = LocationListenerCompat { onFix(it) }

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
            ui = ui.copy(targets = listOfNotNull(point) + list, target = ui.target ?: list.firstOrNull())
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
        getSystemService(LocationManager::class.java)?.let { LocationManagerCompat.removeUpdates(it, listener) }
        super.onStop()
    }

    /** Fixes every second while the screen is shown (interaction), none otherwise. */
    @SuppressLint("MissingPermission")
    private fun listen() {
        val lm = getSystemService(LocationManager::class.java) ?: return
        if (!granted(this)) return
        val req = LocationRequestCompat.Builder(FIX_MS).setQuality(LocationRequestCompat.QUALITY_HIGH_ACCURACY).build()
        LocationManagerCompat.requestLocationUpdates(lm, LocationManager.GPS_PROVIDER, req, listener, mainLooper)
    }

    private fun onFix(l: Location) {
        here = l
        compass.setPlace(l.latitude, l.longitude, l.altitude, l.time)
        ui = ui.withBearing()
    }

    @Suppress("ReturnCount")
    private fun FogUi.withBearing(): FogUi {
        val h = here ?: return copy(bearing = null)
        val t = target ?: return copy(bearing = null)
        val heading = compass.headingDeg ?: if (h.hasBearing() && h.speed > MOVING_MPS) h.bearing.toDouble() else null
        return copy(bearing = fogBearing(h.longitude, h.latitude, t.lon, t.lat, heading, Outings.lang()))
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
            TargetRow(t, selected = t == ui.target) { onPick(t) }
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

@Composable
private fun TargetRow(
    t: FogTarget,
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
    Row(
        Modifier.fillMaxWidth().heightIn(min = GloveTarget).selectable(selected, role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null)
        Text("${t.name} · $kind", style = MaterialTheme.typography.bodyLarge)
    }
}

private const val ARROW_TIP = 0.85f
private const val RING_PX = 4f
private const val MOVING_MPS = 0.6f
private const val ARROW_HALF = 0.45f
private const val ARROW_BACK = 0.6f
private const val ARROW_NOTCH = 0.3f
