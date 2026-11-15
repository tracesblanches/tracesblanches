package fr.skimap.app.offpiste

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.maplibre.android.maps.Style
import org.maplibre.android.style.layers.LineLayer
import org.maplibre.android.style.layers.Property
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.geojson.LineString
import org.maplibre.geojson.Point

/**
 * The off-piste route on the main map (V1): a runtime source and a line drawn on top of
 * the validated style (dashed, the colour of the off-piste routes of the SRN style), kept
 * until another route replaces it or it is cleared.
 */
object RouteOverlay {
    const val SOURCE = "skimap-offpiste-route"
    const val LAYER = "skimap-offpiste-route-line"
    private val mutable = MutableStateFlow<List<Double>?>(null)
    val line: StateFlow<List<Double>?> = mutable.asStateFlow()

    /** Shows [line] (`[lon, lat, …]`). */
    fun show(line: List<Double>) {
        mutable.value = line
    }

    fun clear() {
        mutable.value = null
    }

    /** GeoJSON of a line. */
    fun geoJson(line: List<Double>): String =
        LineString
            .fromLngLats(line.chunked(2).filter { it.size == 2 }.map { Point.fromLngLat(it[0], it[1]) })
            .toJson()

    /** Applies the route (or none) to a loaded style. */
    fun apply(
        style: Style,
        line: List<Double>?,
    ) {
        if (line == null || line.size < MIN_COORDS) {
            style.getLayer(LAYER)?.setProperties(PropertyFactory.visibility(Property.NONE))
            return
        }
        val json = geoJson(line)
        val src = style.getSourceAs<GeoJsonSource>(SOURCE)
        if (src == null) {
            style.addSource(GeoJsonSource(SOURCE, json))
            style.addLayer(
                LineLayer(LAYER, SOURCE).withProperties(
                    PropertyFactory.lineColor(COLOR),
                    PropertyFactory.lineWidth(WIDTH),
                    PropertyFactory.lineDasharray(arrayOf(DASH, GAP)),
                    PropertyFactory.lineCap(Property.LINE_CAP_ROUND),
                ),
            )
        } else {
            src.setGeoJson(json)
            style.getLayer(LAYER)?.setProperties(PropertyFactory.visibility(Property.VISIBLE))
        }
    }

    private const val MIN_COORDS = 4
    private const val COLOR = "#7a1fa2"
    private const val WIDTH = 5f
    private const val DASH = 2f
    private const val GAP = 1f
}
