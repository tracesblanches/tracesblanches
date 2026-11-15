package fr.skimap.app.data

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationManager
import android.os.Build
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import androidx.core.os.CancellationSignal
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

/** One position, once, to find the nearest area. No tracking (that is K4). */
fun interface Locator {
    /** `null` without permission or without any fix within the provider's timeout. */
    suspend fun locate(): Location?
}

/** [Locator] on the platform location manager, coarse precision. */
class PlatformLocator(
    private val context: Context,
) : Locator {
    override suspend fun locate(): Location? {
        val granted =
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) ==
                PackageManager.PERMISSION_GRANTED
        val lm = context.getSystemService(LocationManager::class.java)
        return if (granted && lm != null) current(lm) else null
    }

    private suspend fun current(lm: LocationManager): Location? {
        val provider =
            when {
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && lm.hasProvider(LocationManager.FUSED_PROVIDER) -> {
                    LocationManager.FUSED_PROVIDER
                }

                lm.isProviderEnabled(LocationManager.NETWORK_PROVIDER) -> {
                    LocationManager.NETWORK_PROVIDER
                }

                else -> {
                    LocationManager.GPS_PROVIDER
                }
            }
        return suspendCancellableCoroutine { cont ->
            val cancel = CancellationSignal()
            cont.invokeOnCancellation { cancel.cancel() }
            try {
                LocationManagerCompat.getCurrentLocation(lm, provider, cancel, ContextCompat.getMainExecutor(context)) {
                    cont.resume(it)
                }
            } catch (_: SecurityException) {
                cont.resume(null)
            }
        }
    }
}
