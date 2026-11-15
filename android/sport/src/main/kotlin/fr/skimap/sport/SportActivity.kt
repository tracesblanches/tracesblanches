package fr.skimap.sport

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.core.net.toUri
import androidx.health.connect.client.PermissionController
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch

/**
 * « Envoyer mes sorties » (K11), and the two entries Android and Strava come back by:
 * the Strava callback (`skimap://strava/callback`, redirected by skimap-server; the
 * activity is `singleTask`, so the browser closes and the screen is found again) and the
 * Health Connect rationale (`androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE`, and
 * `VIEW_PERMISSION_USAGE` on Android 14+). The app subclasses it for its theme
 * ([Themed]) and declares it in its manifest.
 */
open class SportActivity : ComponentActivity() {
    private val vm: SportViewModel by viewModels()

    /** The app's theme around the screens; Material's by default. */
    @Composable
    protected open fun Themed(content: @Composable () -> Unit) {
        MaterialTheme(colorScheme = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme(), content = content)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val rationale = intent?.action in RATIONALE_ACTIONS
        if (savedInstanceState == null) callback(intent)
        setContent {
            Themed {
                Surface {
                    if (rationale) HealthRationale(onClose = ::finish) else Screen(vm)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        callback(intent)
    }

    override fun onResume() {
        super.onResume()
        vm.refresh()
    }

    private fun callback(i: Intent?) {
        val uri = i?.data ?: return
        if (uri.toString().startsWith(StravaClient.CALLBACK)) vm.callback(uri)
    }

    companion object {
        /** Health Connect asks the app to explain its use of the data (rationale, privacy policy). */
        const val ACTION_RATIONALE = "androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE"
        private val RATIONALE_ACTIONS = setOf(ACTION_RATIONALE, Intent.ACTION_VIEW_PERMISSION_USAGE)

        /** Health Connect's page on the Play Store (Android 13 and older). */
        private const val HC_STORE = "market://details?id=com.google.android.apps.healthdata"

        fun open(
            context: Context,
            activity: Class<out SportActivity>,
        ) {
            context.startActivity(Intent(context, activity))
        }

        internal fun browse(
            context: Context,
            url: String,
        ) {
            try {
                context.startActivity(Intent(Intent.ACTION_VIEW, url.toUri()))
            } catch (_: ActivityNotFoundException) {
                // No browser: nothing to open (the screen stays as it was).
            }
        }

        internal fun healthStore(context: Context) = browse(context, HC_STORE)
    }
}

@Composable
private fun Screen(vm: SportViewModel) {
    val state by vm.state.collectAsStateWithLifecycle()
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()
    val ask =
        rememberLauncherForActivityResult(PermissionController.createRequestPermissionResultContract()) { granted ->
            vm.healthAnswered(granted)
        }
    SportScreen(
        state = state,
        actions =
            SportActions(
                allowHealth = { ask.launch(HealthConnectSender.PERMISSIONS) },
                installHealth = { SportActivity.healthStore(context) },
                connectStrava = {
                    vm.clearError()
                    scope.launch { vm.authorizeUrl()?.let { SportActivity.browse(context, it) } }
                },
                disconnectStrava = vm::disconnect,
                setAutoSend = vm::setAutoSend,
                close = { (context as? ComponentActivity)?.finish() },
            ),
    )
}
