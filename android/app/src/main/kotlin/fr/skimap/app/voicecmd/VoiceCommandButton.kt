package fr.skimap.app.voicecmd

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.IBinder
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.core.content.ContextCompat
import fr.skimap.app.R
import fr.skimap.app.guidance.SkiTrackingService
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The microphone of the guidance banner, for a phone without headset: one voice command
 * per touch. Shown only once the model is there (downloaded on its own,
 * [VoiceModelWorker]): no setting, no download to accept. The microphone permission is
 * the system's, asked at the first touch (Android cannot grant it without the user).
 */
@Composable
fun VoiceCommandButton(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    // Off the main thread (StrictMode, E1): the files of the model are looked at once.
    var controller by remember { mutableStateOf<VoiceCommandController?>(null) }
    LaunchedEffect(Unit) {
        controller =
            withContext(Dispatchers.IO) {
                VoiceCommandController(context.applicationContext).takeIf { it.installed }
            }
    }
    val c = controller ?: return
    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) askNow(context, c)
        }
    val label = stringResource(R.string.voicecmd_ask)
    IconButton(
        onClick = {
            if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                askNow(context, c)
            } else {
                permission.launch(Manifest.permission.RECORD_AUDIO)
            }
        },
        modifier = modifier.semantics { contentDescription = label },
    ) {
        Icon(painterResource(R.drawable.ic_mic), contentDescription = null)
    }
}

/** One command now: through the location service when an outing runs, else alone. */
fun askNow(
    context: Context,
    controller: VoiceCommandController,
) {
    if (Tracking.state.value !is TrackingState.Running) {
        controller.ask(null)
        return
    }
    val app = context.applicationContext
    val conn =
        object : ServiceConnection {
            override fun onServiceConnected(
                name: ComponentName?,
                binder: IBinder?,
            ) {
                val service = (binder as? TrackingService.Local)?.service
                app.unbindService(this)
                controller.ask(service)
            }

            override fun onServiceDisconnected(name: ComponentName?) = Unit
        }
    // No BIND_AUTO_CREATE: the service runs during the outing, never started from here.
    if (!app.bindService(Intent(app, SkiTrackingService::class.java), conn, 0)) controller.ask(null)
}
