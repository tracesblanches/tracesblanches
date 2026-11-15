package fr.skimap.app.prep

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * What the plan screen adds only when it matters, nothing otherwise: the battery when it
 * will not last the outing with the emergency reserve (C9, K8), the state of the server
 * alert when the plan was sent to it (S2).
 */
@Composable
fun PlanExtras(
    endMs: Long,
    runningDir: String?,
    planDir: String,
) {
    Column {
        BatteryNote(endMs, runningDir, Modifier.padding(top = 12.dp))
        ReturnAlertStatus(planDir)
    }
}
