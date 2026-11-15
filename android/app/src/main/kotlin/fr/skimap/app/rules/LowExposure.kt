package fr.skimap.app.rules

import android.content.Context
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.edit
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.skimap.app.R
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.maplibre.android.maps.Style
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.FillLayer
import org.maplibre.android.style.layers.Property
import org.maplibre.android.style.layers.PropertyFactory

/**
 * Less exposed terrain (V7 #70, F7): class 1 « terrain peu exposé (modèle) » of the
 * avalanche exposure layer of the packs, which the validated styles do not draw (they draw
 * « exposé » and « très exposé »). Shown on demand, on the source the style already reads,
 * under the exposed classes, always with the warning [Warning]: a model output, never a
 * guarantee, never « sans danger ».
 */
object LowExposure {
    const val LAYER_PREFIX = "skimap-low-exposure"
    private const val EXPOSED_LAYER = "exposure-2"
    private val mutable = MutableStateFlow(false)
    val shown: StateFlow<Boolean> = mutable.asStateFlow()

    fun load(context: Context) {
        mutable.value = prefs(context).getBoolean(KEY, false)
    }

    fun set(
        context: Context,
        on: Boolean,
    ) {
        mutable.value = on
        prefs(context).edit { putBoolean(KEY, on) }
    }

    private fun prefs(c: Context) = c.getSharedPreferences("skimap_tools", Context.MODE_PRIVATE)

    /** Adds (once) or toggles the low-exposure fill of every pack of a loaded style. */
    fun apply(
        style: Style,
        on: Boolean,
    ) {
        val exposed = style.layers.filter { it.id == EXPOSED_LAYER || it.id.startsWith("$EXPOSED_LAYER~") }
        for (l in exposed.filterIsInstance<FillLayer>()) {
            val suffix = l.id.removePrefix(EXPOSED_LAYER)
            val id = LAYER_PREFIX + suffix
            if (style.getLayer(id) == null) {
                val fill =
                    FillLayer(id, l.sourceId).withProperties(
                        PropertyFactory.fillColor(COLOR),
                        PropertyFactory.fillOpacity(OPACITY),
                    )
                fill.sourceLayer = l.sourceLayer
                fill.setFilter(Expression.eq(Expression.get("class"), Expression.literal(1)))
                style.addLayerBelow(fill, l.id)
            }
            style.getLayer(id)?.setProperties(PropertyFactory.visibility(if (on) Property.VISIBLE else Property.NONE))
        }
    }

    private const val KEY = "low_exposure"
    private const val COLOR = "#2e7d32"
    private const val OPACITY = 0.3f
}

/** Test tag of the warning. */
const val LOW_EXPOSURE_WARNING = "low_exposure_warning"

/** The warning shown over the map while the less exposed terrain is drawn. */
@Composable
fun LowExposureWarning(modifier: Modifier = Modifier) {
    val on by LowExposure.shown.collectAsStateWithLifecycle()
    if (!on) return
    Surface(
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = modifier.testTag(LOW_EXPOSURE_WARNING),
    ) {
        Text(stringResource(R.string.low_exposure_warning), style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(8.dp))
    }
}
