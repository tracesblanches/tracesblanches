package fr.skimap.app.ui.domains

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/** Full-screen area picker and downloads; asks for the coarse location only when "nearest" is touched. */
@Composable
fun DomainPickerDialog(
    viewModel: DomainsViewModel,
    onPick: (String) -> Unit,
    onClose: () -> Unit,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    LaunchedEffect(Unit) { viewModel.open() }
    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) viewModel.nearest() else viewModel.permissionDenied()
        }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Surface(Modifier.fillMaxSize()) {
            DomainPicker(
                state = state,
                actions =
                    DomainActions(
                        onQuery = viewModel::search,
                        onNearest = {
                            val coarse = Manifest.permission.ACCESS_COARSE_LOCATION
                            if (ContextCompat.checkSelfPermission(context, coarse) == PackageManager.PERMISSION_GRANTED) {
                                viewModel.nearest()
                            } else {
                                permission.launch(coarse)
                            }
                        },
                        onPick = { dir ->
                            onPick(dir)
                            onClose()
                        },
                        onClose = onClose,
                        onDownload = viewModel::download,
                        onCancel = viewModel::cancel,
                        onRemove = viewModel::remove,
                        onMapTouch = viewModel::pickOnMap,
                    ),
            )
        }
    }
}
