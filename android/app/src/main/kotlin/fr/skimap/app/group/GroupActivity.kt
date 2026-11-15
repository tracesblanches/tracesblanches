package fr.skimap.app.group

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.group.GroupActions
import fr.skimap.group.GroupController
import fr.skimap.group.GroupScreen
import fr.skimap.group.GroupStore
import fr.skimap.group.GroupUi
import fr.skimap.group.Groups
import fr.skimap.tracking.Tracking

/** The group screen (N2): create, show the QR code, scan one, the members during the outing. */
class GroupActivity : ComponentActivity() {
    private var afterCamera: (() -> Unit)? = null

    private val camera =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
            if (ok) afterCamera?.invoke() else toast(fr.skimap.group.R.string.group_camera_needed)
            afterCamera = null
        }

    private val nearby =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { r ->
            if (r.values.any { !it }) toast(fr.skimap.group.R.string.group_bluetooth_needed)
        }

    private fun toast(id: Int) = Toast.makeText(this, id, Toast.LENGTH_LONG).show()

    private fun granted(p: String) = ContextCompat.checkSelfPermission(this, p) == PackageManager.PERMISSION_GRANTED

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val controller =
            GroupController(
                RustGroupBackend(),
                GroupStore(applicationContext),
                { RustGroupBackend.planOfTheDay(applicationContext, Tracking.activePlan(applicationContext)) },
                lifecycleScope,
            )
        controller.load()
        val actions =
            GroupActions(
                create = controller::create,
                scanned = controller::scanned,
                choose = controller::choose,
                leave = controller::leave,
                back = ::finish,
                wantCamera = { open ->
                    if (granted(Manifest.permission.CAMERA)) {
                        open()
                    } else {
                        afterCamera = open
                        camera.launch(Manifest.permission.CAMERA)
                    }
                },
            )
        setContent {
            SkimapTheme {
                val ui by controller.ui.collectAsStateWithLifecycle()
                val live by Groups.live.collectAsStateWithLifecycle()
                // In a group: « Nearby devices » to advertise and scan during the outing.
                LaunchedEffect(ui is GroupUi.InGroup) {
                    if (ui is GroupUi.InGroup && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        val wanted = listOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_ADVERTISE)
                        if (wanted.any { !granted(it) }) nearby.launch(wanted.toTypedArray())
                    }
                }
                Surface(Modifier.fillMaxSize()) {
                    GroupScreen(ui, live, controller.hasPlan, actions)
                }
            }
        }
    }
}
