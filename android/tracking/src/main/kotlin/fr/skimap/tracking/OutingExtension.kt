package fr.skimap.tracking

import android.content.Context
import android.location.Location
import android.os.Handler

/**
 * Something that runs inside the location service during an outing, on its thread, with
 * no service, thread, wakelock or timer of its own (the group mode, N2: BLE advertising
 * and batched scan). The app's service lists them ([TrackingService.extensions]).
 */
interface OutingExtension {
    /** What an extension may ask of the service. */
    fun interface Output {
        /** A safety alert: said (Android voice) and vibrated like the core's safety alerts. */
        fun alert(text: String)
    }

    /** The outing starts; [handler] is the service's thread. */
    fun start(
        context: Context,
        handler: Handler,
        output: Output,
    )

    /** After each call to the core: its [batch], and the last [fix] when it came from a GPS batch. */
    fun onBatch(
        batch: Batch,
        fix: Location?,
    )

    /** The outing ends. */
    fun stop()
}
