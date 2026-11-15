package fr.skimap.app.ui.map

import android.graphics.RectF
import android.os.Process
import android.os.SystemClock
import android.util.Log
import android.view.View
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import fr.skimap.app.data.OpenedPack
import fr.skimap.core.Bounds
import fr.skimap.core.Card
import fr.skimap.core.featureCard
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.layers.Property
import org.maplibre.android.style.layers.PropertyFactory
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine

/** A map style fully rendered for the first time (logcat, instrumented test). */
data class MapRendered(
    /** `name` of the style (`skimap <site> <basemap>`). */
    val styleName: String,
    /** From `setStyle` to the first fully rendered frame. */
    val sinceStyleMs: Long,
    /** From the process start to the first fully rendered frame. */
    val sinceProcessStartMs: Long,
)

/** The live map, for the instrumented test: rendered styles and the map itself. */
object MapProbe {
    private val mutableRendered = MutableStateFlow<MapRendered?>(null)
    val rendered: StateFlow<MapRendered?> = mutableRendered.asStateFlow()

    @Volatile
    var map: MapLibreMap? = null
        internal set

    /** Number of `setStyle` calls since the process started: only a pack or basemap change. */
    @Volatile
    var styleLoads: Int = 0
        private set

    internal fun styleLoaded() {
        styleLoads++
    }

    internal fun rendered(r: MapRendered) {
        mutableRendered.value = r
    }
}

/**
 * Automatic view of the map (K12, #115). Until the position is followed (K4), the automatic
 * view is the whole site. [moved] turns true when the user moves the map with a gesture,
 * which shows the "Vue auto" button; [recenter] brings the automatic view back.
 */
class MapController {
    private var map: MapLibreMap? = null
    private var bounds: Bounds? = null
    private val mutableMoved = MutableStateFlow(false)
    val moved: StateFlow<Boolean> = mutableMoved.asStateFlow()

    private val gestureListener =
        MapLibreMap.OnCameraMoveStartedListener { reason ->
            if (reason == MapLibreMap.OnCameraMoveStartedListener.REASON_API_GESTURE) mutableMoved.value = true
        }

    internal fun attach(
        map: MapLibreMap,
        bounds: Bounds,
    ) {
        if (this.map !== map) {
            this.map?.removeOnCameraMoveStartedListener(gestureListener)
            map.addOnCameraMoveStartedListener(gestureListener)
        }
        this.map = map
        this.bounds = bounds
        mutableMoved.value = false
    }

    internal fun detach() {
        map?.removeOnCameraMoveStartedListener(gestureListener)
        map = null
    }

    /** Back to the automatic view: the whole site. */
    fun recenter() {
        val m = map ?: return
        val b = bounds ?: return
        mutableMoved.value = false
        m.animateCamera(CameraUpdateFactory.newLatLngBounds(b.toLatLngBounds(), FIT_PADDING_PX), ANIMATION_MS)
    }

    internal companion object {
        const val ANIMATION_MS = 350
        const val FIT_PADDING_PX = 48
    }
}

internal fun Bounds.toLatLngBounds(): LatLngBounds =
    LatLngBounds
        .Builder()
        .include(LatLng(south, west))
        .include(LatLng(north, east))
        .build()

private suspend fun MapView.awaitMap(): MapLibreMap = suspendCoroutine { c -> getMapAsync { c.resume(it) } }

private const val TAG = "skimap"
private const val TOUCH_RADIUS_DP = 24f

/**
 * MapLibre Native map of an opened pack: the style composed by the Rust core (basemap
 * PMTiles read from the pack, pack layers on top), the symbols drawn by [SymbolBitmaps].
 * Rotation and tilt are off: the map stays north up, and no gesture is required since
 * [controller] zooms and recentres.
 */
@Composable
fun PackMap(
    opened: OpenedPack,
    controller: MapController,
    modifier: Modifier = Modifier,
    hidden: Set<String> = emptySet(),
    onCard: (Card) -> Unit = {},
) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val density = LocalDensity.current.density
    val resources = LocalResources.current
    val timer = remember { RenderTimer() }
    var loaded by remember { mutableStateOf<Style?>(null) }
    val mapView =
        remember {
            MapView(context, MapEngine.mapOptions()).apply {
                onCreate(null)
                // The map is described in French by its Compose container; MapLibre's own
                // English description would be read too.
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                addOnDidFinishRenderingMapListener { fully -> if (fully) timer.rendered() }
            }
        }
    DisposableEffect(lifecycle, mapView) {
        val observer =
            LifecycleEventObserver { _, event ->
                when (event) {
                    Lifecycle.Event.ON_START -> mapView.onStart()
                    Lifecycle.Event.ON_RESUME -> mapView.onResume()
                    Lifecycle.Event.ON_PAUSE -> mapView.onPause()
                    Lifecycle.Event.ON_STOP -> mapView.onStop()
                    else -> Unit
                }
            }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            controller.detach()
            MapProbe.map = null
            mapView.onDestroy()
        }
    }
    AndroidView(factory = { mapView }, modifier = modifier)

    val style = opened.style
    LaunchedEffect(style.json) {
        val map = mapView.awaitMap()
        val dpi = resources.displayMetrics.densityDpi
        // Symbol bitmaps off the main thread; only setStyle runs on it.
        val images = withContext(Dispatchers.Default) { opened.symbols.map { it.id to SymbolBitmaps.draw(it, density, dpi) } }
        val builder = Style.Builder().fromJson(style.json)
        images.forEach { (id, bitmap) -> builder.withImage(id, bitmap) }
        timer.start(styleName(opened))
        loaded = null
        map.setStyle(builder) { loaded = it }
        MapProbe.styleLoaded()
        MapProbe.map = map
    }
    // Panel "Couches": layer visibility only, the style is not reloaded.
    LaunchedEffect(loaded, hidden) {
        val s = loaded ?: return@LaunchedEffect
        for (t in style.toggles) {
            val v = if (t.id in hidden) Property.NONE else Property.VISIBLE
            t.styleLayers.forEach { id -> s.getLayer(id)?.setProperties(PropertyFactory.visibility(v)) }
        }
    }
    // Touch on a refuge: its card. One query per touch, around the finger (24 dp).
    val currentOpened by rememberUpdatedState(opened)
    val currentOnCard by rememberUpdatedState(onCard)
    LaunchedEffect(mapView) {
        val map = mapView.awaitMap()
        val radius = TOUCH_RADIUS_DP * density
        map.addOnMapClickListener { latLng ->
            val p = map.projection.toScreenLocation(latLng)
            val box = RectF(p.x - radius, p.y - radius, p.x + radius, p.y + radius)
            val hit =
                currentOpened.style.overlays
                    .asSequence()
                    .filter { it.packLayer.startsWith("refuges-") }
                    .flatMap { o -> o.styleLayers.asSequence().map { o.packLayer to it } }
                    .firstNotNullOfOrNull { (packLayer, layer) ->
                        map.queryRenderedFeatures(box, layer).firstOrNull()?.let { packLayer to it }
                    }
            val card = hit?.let { (packLayer, f) -> featureCard(packLayer, f.properties()?.toString() ?: "{}") }
            if (card != null) currentOnCard(card)
            card != null
        }
    }
    LaunchedEffect(opened.info.dir) {
        val map = mapView.awaitMap()
        val b = opened.info.bounds
        controller.attach(map, b)
        // Keep the camera over the site: no getting lost by a stray swipe of a glove.
        map.setLatLngBoundsForCameraTarget(b.toLatLngBounds())
        map.moveCamera(CameraUpdateFactory.newLatLngBounds(b.toLatLngBounds(), MapController.FIT_PADDING_PX))
    }
}

/** Name of the style of [opened], as the Rust core writes it (`skimap <site> <basemap>`). */
fun styleName(opened: OpenedPack): String = "skimap ${opened.info.siteId} ${opened.style.basemap ?: "blank"}"

/** Time from `setStyle` to the first fully rendered frame of that style. */
private class RenderTimer {
    private var pending: String? = null
    private var started = 0L

    fun start(name: String) {
        pending = name
        started = SystemClock.uptimeMillis()
    }

    fun rendered() {
        val name = pending ?: return
        pending = null
        val now = SystemClock.uptimeMillis()
        val r = MapRendered(name, now - started, now - Process.getStartUptimeMillis())
        Log.i(TAG, "map ${r.styleName} fully rendered in ${r.sinceStyleMs} ms (${r.sinceProcessStartMs} ms since start)")
        MapProbe.rendered(r)
    }
}
