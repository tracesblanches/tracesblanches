package fr.skimap.community

import android.content.res.Resources
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import org.json.JSONArray
import org.json.JSONObject
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.CircleLayer
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.layers.SymbolLayer
import org.maplibre.android.style.sources.GeoJsonSource

/** GeoJSON of the reports: kind colour and « Glace · il y a 3 h » as properties. */
fun reportsGeoJson(
    reports: List<ServerReport>,
    res: Resources,
    nowMs: Long,
): String {
    val features = JSONArray()
    reports.forEach { r ->
        features.put(
            JSONObject()
                .put("type", "Feature")
                .put("geometry", JSONObject().put("type", "Point").put("coordinates", JSONArray().put(r.lon).put(r.lat)))
                .put(
                    "properties",
                    JSONObject()
                        .put("id", r.id)
                        .put("color", "#%06X".format(r.kind.color and RGB_MASK))
                        .put("label", res.getString(r.kind.label) + " · " + ageText(res, nowMs, r.seenAtMs)),
                ),
        )
    }
    return JSONObject().put("type", "FeatureCollection").put("features", features).toString()
}

/** First `text-font` of a style (the fonts its glyphs hold), for the labels. */
fun styleFont(styleJson: String): Array<String>? =
    runCatching {
        val layers = JSONObject(styleJson).getJSONArray("layers")
        (0 until layers.length())
            .asSequence()
            .mapNotNull { layers.getJSONObject(it).optJSONObject("layout")?.optJSONArray("text-font") }
            .firstOrNull()
            ?.let { a -> Array(a.length()) { a.getString(it) } }
    }.getOrNull()

private const val RGB_MASK = 0xFFFFFFL
private const val SOURCE = "community-reports"
private const val MAP_HEIGHT_DP = 280
private const val CIRCLE_RADIUS = 9f
private const val CIRCLE_STROKE = 2f
private const val TEXT_SIZE = 13f
private const val TEXT_OFFSET = 1.2f
private const val PADDING_PX = 40

/**
 * The reports around on the pack's map (offline style of the core), each a coloured dot
 * labelled with its kind and age.
 */
@Composable
fun ReportsMap(
    area: AreaContext,
    reports: List<ServerReport>,
    nowMs: Long,
    modifier: Modifier = Modifier,
) {
    val style = area.styleJson ?: return
    val context = LocalContext.current
    val res = LocalResources.current
    val view = remember { MapView(context).also { it.onCreate(null) } }
    val holder = remember { arrayOfNulls<MapLibreMap>(1) }
    DisposableEffect(view) {
        view.onStart()
        view.onResume()
        onDispose {
            view.onPause()
            view.onStop()
            view.onDestroy()
        }
    }
    AndroidView(
        factory = {
            view.getMapAsync { map ->
                holder[0] = map
                map.uiSettings.isRotateGesturesEnabled = false
                map.uiSettings.isTiltGesturesEnabled = false
                map.setStyle(Style.Builder().fromJson(style)) { s ->
                    s.addSource(GeoJsonSource(SOURCE, reportsGeoJson(reports, res, nowMs)))
                    s.addLayer(
                        CircleLayer("$SOURCE-dots", SOURCE).withProperties(
                            PropertyFactory.circleColor(Expression.get("color")),
                            PropertyFactory.circleRadius(CIRCLE_RADIUS),
                            PropertyFactory.circleStrokeColor("#FFFFFF"),
                            PropertyFactory.circleStrokeWidth(CIRCLE_STROKE),
                        ),
                    )
                    styleFont(style)?.let { font ->
                        s.addLayer(
                            SymbolLayer("$SOURCE-labels", SOURCE).withProperties(
                                PropertyFactory.textField(Expression.get("label")),
                                PropertyFactory.textFont(font),
                                PropertyFactory.textSize(TEXT_SIZE),
                                PropertyFactory.textOffset(arrayOf(0f, TEXT_OFFSET)),
                                PropertyFactory.textAnchor("top"),
                                PropertyFactory.textHaloColor("#FFFFFF"),
                                PropertyFactory.textHaloWidth(CIRCLE_STROKE),
                            ),
                        )
                    }
                    map.moveCamera(
                        CameraUpdateFactory.newLatLngBounds(
                            area.bbox.let { (west, south, east) -> LatLngBounds.from(area.bbox.last(), east, south, west) },
                            PADDING_PX,
                        ),
                    )
                }
            }
            view
        },
        update = {
            holder[0]?.style?.getSourceAs<GeoJsonSource>(SOURCE)?.setGeoJson(reportsGeoJson(reports, res, nowMs))
        },
        modifier = modifier.fillMaxWidth().height(MAP_HEIGHT_DP.dp),
    )
}
