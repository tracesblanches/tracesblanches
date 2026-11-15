package fr.skimap.app.prep

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.core.content.edit
import fr.skimap.core.BatteryView
import fr.skimap.core.batteryForecast
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** The battery now: level (%), temperature (°C). */
data class BatteryNow(
    val levelPct: Double,
    val tempC: Double,
)

/** Battery forecast (C9, K8) for the screens: the platform's sticky battery state, the core's model. */
object Battery {
    /** The battery now (the sticky broadcast: no receiver kept), `null` if unknown. */
    @Suppress("ReturnCount")
    fun now(context: Context): BatteryNow? {
        val i = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED)) ?: return null
        val level = i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = i.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
        if (level < 0 || scale <= 0) return null
        // Tenths of a degree (HAL).
        val temp = i.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, DEFAULT_TENTHS) / TENTHS
        return BatteryNow(level * PERCENT / scale, temp)
    }

    /**
     * The forecast for an outing ending at [endMs] (`null`: none planned); [outing]: its
     * directory, whose level at the start is kept to measure the drain.
     */
    fun forecast(
        context: Context,
        endMs: Long?,
        outing: String?,
        nowMs: Long = System.currentTimeMillis(),
    ): BatteryView? {
        val b = now(context) ?: return null
        var measuredH = 0.0
        var measuredPct = 0.0
        if (outing != null) {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            if (p.getString(KEY_DIR, null) != outing) {
                p.edit {
                    putString(KEY_DIR, outing)
                    putLong(KEY_T, nowMs)
                    putFloat(KEY_LEVEL, b.levelPct.toFloat())
                }
            } else {
                measuredH = (nowMs - p.getLong(KEY_T, nowMs)) / MS_PER_H
                measuredPct = p.getFloat(KEY_LEVEL, b.levelPct.toFloat()) - b.levelPct
            }
        }
        return batteryForecast(nowMs, b.levelPct, b.tempC, 0.0, endMs ?: 0L, SCREEN_MIN_PER_H, 0.0, false, measuredH, measuredPct)
    }

    private const val PREFS = "skimap_battery"
    private const val KEY_DIR = "outing"
    private const val KEY_T = "start_ms"
    private const val KEY_LEVEL = "start_level"
    private const val DEFAULT_TENTHS = 200
    private const val TENTHS = 10.0
    private const val PERCENT = 100.0
    private const val MS_PER_H = 3_600_000.0
    private const val SCREEN_MIN_PER_H = 3.0
}

/** Test tag of the battery note. */
const val BATTERY_NOTE = "battery_note"

/** « Batterie suffisante jusqu'à … », the reserve, and the warning when the outing goes beyond. */
@Composable
fun BatteryNote(
    endMs: Long?,
    outing: String?,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    var view by remember { mutableStateOf<BatteryView?>(null) }
    LaunchedEffect(endMs, outing) { view = withContext(Dispatchers.IO) { Battery.forecast(context, endMs, outing) } }
    // Shown only when the battery is a problem for the outing.
    view?.takeIf { it.warning != null }?.let { BatteryCard(it, modifier) }
}

/** The battery forecast, stateless. */
@Composable
fun BatteryCard(
    v: BatteryView,
    modifier: Modifier = Modifier,
) {
    val warn = v.warning != null
    Surface(
        color = if (warn) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceContainer,
        contentColor = if (warn) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSurface,
        shape = MaterialTheme.shapes.medium,
        modifier = modifier.fillMaxWidth().testTag(BATTERY_NOTE),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(v.text, style = MaterialTheme.typography.bodyLarge)
            v.warning?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
        }
    }
}
