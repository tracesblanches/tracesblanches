package fr.skimap.app.group

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.skimap.group.Groups
import fr.skimap.group.Member
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.CircleLayer
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.layers.SymbolLayer
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.geojson.Feature
import org.maplibre.geojson.FeatureCollection
import org.maplibre.geojson.Point

/** Runtime source and layers of the group's members on the map. */
object GroupMapLayer {
    const val SOURCE = "group-members"
    const val DOTS = "group-members-dot"
    const val LABELS = "group-members-label"

    /** A position older than this is drawn faded: it may be far from where the member is. */
    const val FRESH_S = 120L
    private const val MINUTE = 60L
    private const val HOUR = 3600L
    const val DOT_RADIUS = 7f
    const val DOT_STROKE = 2f
    const val STALE_OPACITY = 0.45f
    const val LABEL_SIZE = 13f
    const val LABEL_OFFSET = 1.3f
    const val LABEL_HALO = 1.5f

    /** Blue of a member, red of a member stopped or off the route. */
    val DOT_COLOR = android.graphics.Color.rgb(21, 101, 192)
    val ALERT_COLOR = android.graphics.Color.rgb(198, 40, 40)

    /** « Léa · 6 min », « Jo · 1 h 05 », « Paul · 0 min ». */
    fun label(m: Member): String {
        val age = m.ageS ?: return m.name
        val t = if (age < HOUR) "${age / MINUTE} min" else "${age / HOUR} h %02d".format(java.util.Locale.ROOT, (age % HOUR) / MINUTE)
        return "${m.name} · $t"
    }

    /** The members heard (not this phone), as GeoJSON features. */
    fun features(members: List<Member>): FeatureCollection =
        FeatureCollection.fromFeatures(
            members.filter { !it.me && it.lat != null && it.lon != null }.map { m ->
                Feature.fromGeometry(Point.fromLngLat(m.lon!!, m.lat!!)).apply {
                    addStringProperty("label", label(m))
                    addBooleanProperty("fresh", (m.ageS ?: Long.MAX_VALUE) <= FRESH_S)
                    addBooleanProperty("alert", m.stopped || m.offRoute)
                }
            },
        )
}

/**
 * The group's members on the map during an outing (N2): a dot per member at its last
 * known position (faded when older than 2 min, red when stopped or off the route) and its
 * name with the age of the position. Added to the style once loaded; nothing outside an
 * outing with a group.
 */
@Composable
fun GroupMembersOnMap(style: org.maplibre.android.maps.Style?) {
    val live by Groups.live.collectAsStateWithLifecycle()
    LaunchedEffect(style, live) {
        val s = style ?: return@LaunchedEffect
        if (!s.isFullyLoaded) return@LaunchedEffect
        val members = live?.members.orEmpty()
        var source = s.getSourceAs<GeoJsonSource>(GroupMapLayer.SOURCE)
        if (source == null) {
            if (members.none { !it.me && it.lat != null }) return@LaunchedEffect
            source = GeoJsonSource(GroupMapLayer.SOURCE)
            s.addSource(source)
            s.addLayer(
                CircleLayer(GroupMapLayer.DOTS, GroupMapLayer.SOURCE).withProperties(
                    PropertyFactory.circleRadius(GroupMapLayer.DOT_RADIUS),
                    PropertyFactory.circleColor(
                        Expression.switchCase(
                            Expression.get("alert"),
                            Expression.color(GroupMapLayer.ALERT_COLOR),
                            Expression.color(GroupMapLayer.DOT_COLOR),
                        ),
                    ),
                    PropertyFactory.circleOpacity(
                        Expression.switchCase(
                            Expression.get("fresh"),
                            Expression.literal(1f),
                            Expression.literal(GroupMapLayer.STALE_OPACITY),
                        ),
                    ),
                    PropertyFactory.circleStrokeColor(android.graphics.Color.WHITE),
                    PropertyFactory.circleStrokeWidth(GroupMapLayer.DOT_STROKE),
                ),
            )
            s.addLayer(
                SymbolLayer(GroupMapLayer.LABELS, GroupMapLayer.SOURCE).withProperties(
                    PropertyFactory.textField(Expression.get("label")),
                    // The font of the validated styles (their glyphs come with Plan IGN).
                    PropertyFactory.textFont(arrayOf("Source Sans Pro Semibold")),
                    PropertyFactory.textSize(GroupMapLayer.LABEL_SIZE),
                    PropertyFactory.textOffset(arrayOf(0f, GroupMapLayer.LABEL_OFFSET)),
                    PropertyFactory.textAnchor("top"),
                    PropertyFactory.textHaloColor(android.graphics.Color.WHITE),
                    PropertyFactory.textHaloWidth(GroupMapLayer.LABEL_HALO),
                    PropertyFactory.textAllowOverlap(true),
                ),
            )
        }
        source.setGeoJson(GroupMapLayer.features(members))
    }
}
