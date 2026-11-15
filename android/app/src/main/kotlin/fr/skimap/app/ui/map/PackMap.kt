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
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import fr.skimap.app.data.CameraSpot
import fr.skimap.app.data.OpenedPack
import fr.skimap.app.group.GroupMembersOnMap
import fr.skimap.core.Bounds
import fr.skimap.core.Card
import fr.skimap.core.Lang
import fr.skimap.core.MapStyle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.layers.Property
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.geojson.Feature
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine

/** A map style fully rendered for the first time (logcat, instrumented test). */
data class MapRendered(
    /** `name` of the style (`skimap <site or libre> <style>-<theme>`). */
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

    /** Number of `setStyle` calls since the process started: only a pack or style change. */
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

    /** Highlights [feature] (a touched piste or zone) on the map; `null` clears it. */
    fun select(feature: Feature?) {
        val source = map?.style?.getSourceAs<GeoJsonSource>(SELECTION_SOURCE) ?: return
        if (feature == null) {
            source.setGeoJson(EMPTY_COLLECTION)
        } else {
            source.setGeoJson(Feature.fromGeometry(feature.geometry()))
        }
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

        /** Runtime source of the styles that draws the touched feature (`selection-halo`). */
        const val SELECTION_SOURCE = "selection"
        const val EMPTY_COLLECTION = "{\"type\":\"FeatureCollection\",\"features\":[]}"
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

/** Half the side of the touched square: generous, for a gloved finger. */
private const val TOUCH_RADIUS_DP = 32f

/**
 * MapLibre Native map of the opened packs, in a style validated by the user (`styles/`)
 * instantiated on them by the Rust core: every tile, glyph and icon is read from the
 * packs. Rotation and tilt are off: the map stays north up, and no gesture is required
 * since [controller] zooms and recentres.
 */
@Composable
fun PackMap(
    opened: OpenedPack,
    controller: MapController,
    modifier: Modifier = Modifier,
    hidden: Set<String> = emptySet(),
    onCard: (Card) -> Unit = {},
    /** Where the map was when the app was last left (free mode starts there). */
    lastCamera: CameraSpot? = null,
    /** The camera when the app goes to the background (saved once, not while moving). */
    onLeave: (CameraSpot) -> Unit = {},
) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val density = LocalDensity.current.density
    val lang = cardLang(LocalConfiguration.current.locales[0])
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
    val currentOnLeave by rememberUpdatedState(onLeave)
    DisposableEffect(lifecycle, mapView) {
        val observer = lifecycleObserver(mapView) { currentOnLeave(it) }
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
        timer.start(style.name)
        loaded = null
        map.setStyle(Style.Builder().fromJson(style.json)) { loaded = it }
        MapProbe.styleLoaded()
        MapProbe.map = map
    }
    // Panel "Couches": layer visibility of the groups of the style, no reload.
    LaunchedEffect(loaded, hidden) { loaded?.let { showGroups(it, style, hidden) } }
    // The group's members during an outing (N2).
    GroupMembersOnMap(loaded)
    // Touch on a refuge, a piste or a zone: its card, and the piste or zone highlighted.
    // One query per touch, around the finger.
    val currentOpened by rememberUpdatedState(opened)
    val currentOnCard by rememberUpdatedState(onCard)
    val currentLang by rememberUpdatedState(lang)
    LaunchedEffect(mapView) {
        val map = mapView.awaitMap()
        val radius = TOUCH_RADIUS_DP * density
        map.addOnMapClickListener { latLng ->
            val hit = cardAt(map, latLng, radius, currentOpened, currentLang)
            controller.select(hit?.feature)
            if (hit != null) currentOnCard(hit.card)
            hit != null
        }
    }
    LaunchedEffect(opened.info.siteId, opened.info.bounds) {
        val map = mapView.awaitMap()
        val b = opened.info.bounds
        // Free mode: the automatic view is the area the user was in, else the first pack;
        // the camera may go anywhere over the installed packs.
        val spot = lastCamera?.takeIf { it.isIn(b) }
        val auto = if (spot != null) packBoundsAround(opened, spot) ?: b else firstBounds(opened) ?: b
        controller.attach(map, auto)
        // Keep the camera over the packs: no getting lost by a stray swipe of a glove.
        map.setLatLngBoundsForCameraTarget(b.toLatLngBounds())
        if (spot != null) {
            map.moveCamera(CameraUpdateFactory.newLatLngZoom(LatLng(spot.lat, spot.lon), spot.zoom))
        } else {
            map.moveCamera(CameraUpdateFactory.newLatLngBounds(auto.toLatLngBounds(), MapController.FIT_PADDING_PX))
        }
    }
}

/** Forwards the lifecycle to [mapView]; [onLeave] gets the camera when the app is left. */
private fun lifecycleObserver(
    mapView: MapView,
    onLeave: (CameraSpot) -> Unit,
) = LifecycleEventObserver { _, event ->
    when (event) {
        Lifecycle.Event.ON_START -> {
            mapView.onStart()
        }

        Lifecycle.Event.ON_RESUME -> {
            mapView.onResume()
        }

        Lifecycle.Event.ON_PAUSE -> {
            mapView.onPause()
        }

        Lifecycle.Event.ON_STOP -> {
            MapProbe.map?.cameraPosition?.let { c ->
                c.target?.let { t -> onLeave(CameraSpot(t.latitude, t.longitude, c.zoom)) }
            }
            mapView.onStop()
        }

        else -> {
            Unit
        }
    }
}

/** Panel "Couches": visibility of the switchable groups of [style], without reloading it. */
private fun showGroups(
    loaded: Style,
    style: MapStyle,
    hidden: Set<String>,
) {
    for (g in style.groups.filter { it.toggleable }) {
        val v = if (g.id in hidden) Property.NONE else Property.VISIBLE
        g.styleLayers.forEach { id -> loaded.getLayer(id)?.setProperties(PropertyFactory.visibility(v)) }
    }
}

/** Bounds of the installed pack around [spot] (free mode), if one contains it. */
private fun packBoundsAround(
    opened: OpenedPack,
    spot: CameraSpot,
): Bounds? = opened.packBounds.firstOrNull { spot.isIn(it) }

/** Bounds of the first pack of the set. */
private fun firstBounds(opened: OpenedPack): Bounds? = opened.packBounds.firstOrNull()

/** Name of the style of [opened], as the Rust core writes it (`skimap <site> <style>-<theme>`). */
fun styleName(opened: OpenedPack): String = opened.style.name

/** Language of the cards: French on a French phone, English otherwise (VO6). */
fun cardLang(locale: java.util.Locale): Lang = if (locale.language == "fr") Lang.FR else Lang.EN

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
