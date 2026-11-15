package fr.skimap.community

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.Uri
import android.os.Bundle
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.maplibre.android.MapLibre
import java.io.File
import java.util.Locale

/**
 * Where the app plugs its core (set once in the app's main activity) and the OSM API used
 * (production in the app; tests point it at a local server or the sandbox).
 */
object Community {
    @Volatile
    var core: CommunityCore? = null

    @Volatile
    var osmBase: String = OsmNotes.PRODUCTION
}

/** The screens of the module: report (and around), « Mes sorties », OSM note. */
class CommunityActivity : ComponentActivity() {
    private val mode by lazy { intent.getStringExtra(EXTRA_MODE) ?: MODE_REPORT }
    private val model: CommunityViewModel by viewModels {
        val app = applicationContext
        viewModelFactory {
            initializer {
                val core = checkNotNull(Community.core) { "Community.core not set by the app" }
                val reports =
                    ReportsBackend(
                        outbox = ReportSync.outbox(app),
                        api = ReportSync.api(app),
                        device = { ReportSync.device(app).current() },
                        mine = ReportSync.mine(app),
                        enqueue = { ReportSync.enqueue(app) },
                    )
                CommunityViewModel(
                    CommunityDeps(core, reports, { app.assets.open("challenges.json").use { it.readBytes().decodeToString() } }),
                    intent.getStringExtra(EXTRA_SITE),
                )
            }
        }
    }

    private val osm: OsmViewModel by viewModels {
        viewModelFactory {
            initializer {
                val base = Community.osmBase
                OsmViewModel(
                    checkNotNull(Community.core) { "Community.core not set by the app" },
                    OsmNotes(base),
                    base.toUri().host.orEmpty(),
                    Locale.getDefault().language == "fr",
                )
            }
        }
    }
    private var listener: LocationListener? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val dark = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
        when (mode) {
            MODE_OUTINGS -> {
                model.openOutings()
            }

            else -> {
                if (mode == MODE_REPORT) {
                    MapLibre.getInstance(applicationContext)
                    MapLibre.setConnected(false)
                    model.openReports()
                }
                locate()
            }
        }
        setContent {
            MaterialTheme(colorScheme = if (dark) darkColorScheme() else lightColorScheme()) {
                Surface(Modifier.fillMaxSize()) { Content(Modifier.fillMaxSize().safeDrawingPadding()) }
            }
        }
    }

    override fun onDestroy() {
        listener?.let { (getSystemService(Context.LOCATION_SERVICE) as LocationManager).removeUpdates(it) }
        super.onDestroy()
    }

    @Composable
    private fun Content(modifier: Modifier) {
        val now = System.currentTimeMillis()
        when (mode) {
            MODE_OUTINGS -> {
                val s by model.outings.collectAsStateWithLifecycle()
                val pick =
                    rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
                        if (uri != null) importArchive(uri)
                    }
                OutingsScreen(s, onImport = { pick.launch(arrayOf("application/zip", "application/octet-stream")) }, modifier = modifier)
            }

            MODE_OSM -> {
                val s by osm.state.collectAsStateWithLifecycle()
                OsmNoteScreen(s, osm::setCorrection, osm::setComment, osm::send, modifier = modifier)
            }

            else -> {
                val s by model.report.collectAsStateWithLifecycle()
                val shot =
                    rememberLauncherForActivityResult(ActivityResultContracts.TakePicturePreview()) { bmp ->
                        if (bmp != null) {
                            lifecycleScope.launch {
                                model.attachPhoto(withContext(Dispatchers.Default) { PhotoReducer.reduce(bmp) })
                            }
                        }
                    }
                ReportScreen(
                    s,
                    now,
                    onKind = model::report,
                    onPhoto = { shot.launch(null) },
                    onCancel = model::cancelLast,
                    onVote = model::vote,
                    modifier = modifier,
                    map = { s.area?.let { ReportsMap(it, s.around, now) } },
                )
            }
        }
    }

    /** Copies the archive the user picked into the cache (read on the phone, never sent). */
    private fun importArchive(uri: Uri) {
        lifecycleScope.launch {
            val zip = File(cacheDir, "strava-export.zip")
            withContext(Dispatchers.IO) {
                contentResolver.openInputStream(uri)?.use { input -> zip.outputStream().use { input.copyTo(it) } }
            }
            model.importStrava(zip)
        }
    }

    /** The last known position if recent, else one fix; the gesture never waits for it. */
    private fun locate() {
        val fine = ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        if (!fine) {
            val ask =
                registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
                    if (granted) locate() else position(null)
                }
            ask.launch(Manifest.permission.ACCESS_FINE_LOCATION)
            return
        }
        val lm = getSystemService(Context.LOCATION_SERVICE) as LocationManager
        val now = System.currentTimeMillis()
        val recent =
            listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
                .filter { runCatching { lm.isProviderEnabled(it) }.getOrDefault(false) }
                .mapNotNull { runCatching { lm.getLastKnownLocation(it) }.getOrNull() }
                .filter { now - it.time < RECENT_MS }
                .maxByOrNull { it.time }
        if (recent != null) position(recent.toFix())
        val provider =
            if (lm.isProviderEnabled(
                    LocationManager.GPS_PROVIDER,
                )
            ) {
                LocationManager.GPS_PROVIDER
            } else {
                LocationManager.NETWORK_PROVIDER
            }
        // One fix, then the listener removes itself (no tracking here).
        val once =
            object : LocationListener {
                override fun onLocationChanged(loc: Location) {
                    lm.removeUpdates(this)
                    position(loc.toFix())
                }
            }
        listener = once
        runCatching { lm.requestLocationUpdates(provider, 0L, 0f, once, Looper.getMainLooper()) }
            .onFailure { position(null) }
    }

    private fun position(fix: Fix?) {
        model.setPosition(fix)
        if (mode == MODE_OSM) osm.setPosition(fix)
    }

    private fun Location.toFix() = Fix(longitude, latitude, time)

    companion object {
        const val EXTRA_MODE = "mode"
        const val EXTRA_SITE = "site"
        const val MODE_REPORT = "report"
        const val MODE_OUTINGS = "outings"
        const val MODE_OSM = "osm"
        private const val RECENT_MS = 2 * 60_000L

        fun intent(
            context: Context,
            mode: String,
            site: String?,
        ): Intent = Intent(context, CommunityActivity::class.java).putExtra(EXTRA_MODE, mode).putExtra(EXTRA_SITE, site)
    }
}
