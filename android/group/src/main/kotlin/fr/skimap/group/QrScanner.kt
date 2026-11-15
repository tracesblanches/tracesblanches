package fr.skimap.group

import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/** Decodes a QR code in the luminance plane of a camera frame (ZXing); null if none. */
fun decodeQr(
    luminance: ByteArray,
    width: Int,
    height: Int,
    rowStride: Int = width,
): String? {
    val source = PlanarYUVLuminanceSource(luminance, rowStride, height, 0, 0, width, height, false)
    val reader =
        MultiFormatReader().apply {
            setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE), DecodeHintType.TRY_HARDER to true))
        }
    return try {
        reader.decodeWithState(BinaryBitmap(HybridBinarizer(source))).text
    } catch (_: NotFoundException) {
        null
    }
}

private fun ImageProxy.luminance(): Triple<ByteArray, Int, Int> {
    val plane = planes[0]
    val buf = plane.buffer
    val bytes = ByteArray(buf.remaining())
    buf.get(bytes)
    return Triple(bytes, plane.rowStride, height)
}

/** The camera, until a QR code is read (CameraX preview and analysis, ZXing). */
@Composable
fun QrScanner(
    onText: (String) -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val currentOnText by rememberUpdatedState(onText)
    val preview = remember { PreviewView(context) }
    DisposableEffect(owner) {
        val executor = Executors.newSingleThreadExecutor()
        val done = AtomicBoolean(false)
        val future = ProcessCameraProvider.getInstance(context)
        var provider: ProcessCameraProvider? = null
        future.addListener({
            val p = future.get()
            provider = p
            val usePreview = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
            val analysis =
                ImageAnalysis
                    .Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
            analysis.setAnalyzer(executor) { image ->
                image.use {
                    if (done.get()) return@use
                    val (y, stride, h) = it.luminance()
                    val text = decodeQr(y, it.width, h, stride)
                    if (text != null && done.compareAndSet(false, true)) {
                        ContextCompat.getMainExecutor(context).execute { currentOnText(text) }
                    }
                }
            }
            p.unbindAll()
            p.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, usePreview, analysis)
        }, ContextCompat.getMainExecutor(context))
        onDispose {
            provider?.unbindAll()
            executor.shutdown()
        }
    }
    Box(modifier.fillMaxSize()) {
        AndroidView(factory = { preview }, modifier = Modifier.fillMaxSize())
        Surface(
            Modifier.align(Alignment.TopCenter).padding(24.dp),
            color = MaterialTheme.colorScheme.surface,
            shape = MaterialTheme.shapes.medium,
        ) { Text(stringResource(R.string.group_scan_hint), Modifier.padding(12.dp)) }
        Button(onClick = onCancel, modifier = Modifier.align(Alignment.BottomCenter).padding(32.dp)) {
            Text(stringResource(R.string.group_cancel))
        }
    }
}
