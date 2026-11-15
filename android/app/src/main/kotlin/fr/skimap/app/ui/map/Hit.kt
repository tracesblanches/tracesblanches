package fr.skimap.app.ui.map

import android.graphics.PointF
import android.graphics.RectF
import fr.skimap.app.data.OpenedPack
import fr.skimap.core.Card
import fr.skimap.core.Lang
import fr.skimap.core.featureCard
import fr.skimap.core.pisteCard
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.geojson.Feature

/** A card and the feature it describes (highlighted when it is a line or an area). */
internal class Hit(
    val card: Card,
    val feature: Feature?,
)

/** Pack layers with cards, in the order a touch looks for them: points first, areas last. */
private val TOUCH_ORDER = listOf("refuges-", "stations", "pistes", "avalanche-areas", "zones", "no-network")

private fun touchRank(packLayer: String): Int =
    TOUCH_ORDER.indexOfFirst { if (it.endsWith("-")) packLayer.startsWith(it) else packLayer == it }

/**
 * Card of what is under the finger ([radius] pixels around [latLng]): a refuge or a
 * station, else a piste (with its area and the trail pass of the place), else a known
 * avalanche path, a regulated zone or a zone without network.
 */
internal fun cardAt(
    map: MapLibreMap,
    latLng: LatLng,
    radius: Float,
    opened: OpenedPack,
    lang: Lang,
): Hit? {
    val p = map.projection.toScreenLocation(latLng)
    val box = RectF(p.x - radius, p.y - radius, p.x + radius, p.y + radius)
    val hit =
        opened.style.overlays
            .filter { touchRank(it.packLayer) >= 0 }
            .sortedBy { touchRank(it.packLayer) }
            .asSequence()
            .flatMap { o -> o.styleLayers.asSequence().map { o.packLayer to it } }
            .firstNotNullOfOrNull { (packLayer, layer) ->
                map.queryRenderedFeatures(box, layer).firstOrNull()?.let { Triple(packLayer, layer, it) }
            }
    return hit?.let { (packLayer, layer, f) ->
        val props = f.properties()?.toString() ?: "{}"
        if (packLayer == "pistes") {
            pisteCard(props, domainOf(opened, layer), feesAt(map, p, opened), lang)?.let { Hit(it, f) }
        } else {
            // Points (refuges, stations) are not highlighted: their symbol is the mark.
            featureCard(packLayer, props, lang)?.let { Hit(it, f.takeIf { f.geometry()?.type() != "Point" }) }
        }
    }
}

/** Name of the area of a style layer: its pack (suffix `~p<k>`, none for the first). */
private fun domainOf(
    opened: OpenedPack,
    layer: String,
): String? {
    val k = layer.substringAfterLast('~', "p0").removePrefix("p").toIntOrNull() ?: 0
    return opened.packNames.getOrNull(k)?.takeIf { it.isNotEmpty() }
}

/** Properties of the trail-pass zones drawn at [p]. */
private fun feesAt(
    map: MapLibreMap,
    p: PointF,
    opened: OpenedPack,
): List<String> =
    opened.style.overlays
        .filter { it.packLayer == "fee-zones" }
        .flatMap { it.styleLayers }
        .flatMap { map.queryRenderedFeatures(p, it) }
        .mapNotNull { it.properties()?.toString() }
        .distinct()
