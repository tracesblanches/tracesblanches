package fr.skimap.group

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertisingSet
import android.bluetooth.le.AdvertisingSetCallback
import android.bluetooth.le.AdvertisingSetParameters
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.ParcelUuid
import android.util.Log
import androidx.core.content.ContextCompat

/** The radio of the group mode: advertise two frames, scan for the group's. */
interface GroupRadio {
    /** Whether the radio can work (Bluetooth LE present, on, permitted). */
    val available: Boolean

    /**
     * Scans for frames starting with one of [prefixes]; [onFrames] gets them in batches
     * (on the service's thread). Called again when the prefixes change (every 15 min).
     */
    fun scan(
        prefixes: List<ByteArray>,
        batchDelayMs: Long,
        onFrames: (List<ByteArray>) -> Unit,
    )

    /** Advertises [own] (first set) and [relay] (second set; null: none). */
    fun advertise(
        own: ByteArray?,
        relay: ByteArray?,
    )

    fun stop()
}

/** Service Data AD of a frame, with the 16-bit [uuid] (`0000xxxx-0000-1000-8000-00805f9b34fb`). */
fun serviceUuid(uuid: Int): ParcelUuid =
    ParcelUuid.fromString(String.format(java.util.Locale.ROOT, "0000%04x-0000-1000-8000-00805f9b34fb", uuid))

/**
 * Bluetooth LE of the phone (report 10, §1 and §3): legacy non-connectable,
 * non-scannable advertising (ADV_NONCONN_IND) at INTERVAL_MEDIUM (250 ms; 100 ms « should
 * not be used for continuous background advertising »), one advertising set for this
 * phone's frame and one for the relayed positions, their data replaced at each GPS batch;
 * a scan filtered in the controller on the Service Data prefix (version and rotating tag
 * of the group: the processor wakes only for this group), SCAN_MODE_LOW_POWER, results in
 * batches (`setReportDelay`, aligned on the GPS batches) when the controller batches. The
 * random address of the advertising sets is rotated by Android.
 */
class BleGroupRadio(
    private val context: Context,
    private val handler: Handler,
    uuid: Int,
) : GroupRadio {
    private val parcelUuid = serviceUuid(uuid)
    private val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
    private val sets = arrayOfNulls<AdvertisingSet>(2)
    private val starting = BooleanArray(2)
    private val pending = arrayOfNulls<ByteArray>(2)
    private val callbacks = Array(2) { SetCallback(it) }
    private var scanCallback: ScanCallback? = null

    private fun granted(p: String) = ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED

    private val permitted: Boolean
        get() =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                granted(Manifest.permission.BLUETOOTH_SCAN) && granted(Manifest.permission.BLUETOOTH_ADVERTISE)
            } else {
                granted(Manifest.permission.ACCESS_FINE_LOCATION)
            }

    override val available: Boolean
        get() = adapter != null && adapter.isEnabled && permitted

    private inner class SetCallback(
        val index: Int,
    ) : AdvertisingSetCallback() {
        override fun onAdvertisingSetStarted(
            set: AdvertisingSet?,
            txPower: Int,
            status: Int,
        ) {
            handler.post {
                starting[index] = false
                if (status == ADVERTISE_SUCCESS && set != null) {
                    sets[index] = set
                    // Data that came while the set was starting.
                    pending[index]?.let { write(index, it) }
                } else {
                    Log.w(TAG, "group: advertising set $index failed ($status)")
                }
            }
        }
    }

    private fun data(frame: ByteArray) =
        AdvertiseData
            .Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceData(parcelUuid, frame)
            .build()

    @SuppressLint("MissingPermission")
    private fun write(
        index: Int,
        frame: ByteArray,
    ) {
        try {
            sets[index]?.setAdvertisingData(data(frame))
        } catch (e: SecurityException) {
            Log.w(TAG, "group: advertising permission", e)
        }
    }

    @SuppressLint("MissingPermission")
    override fun advertise(
        own: ByteArray?,
        relay: ByteArray?,
    ) {
        if (!available) return
        val advertiser = adapter?.bluetoothLeAdvertiser ?: return
        listOf(own, relay).forEachIndexed { i, frame ->
            if (frame == null) return@forEachIndexed
            pending[i] = frame
            when {
                sets[i] != null -> {
                    write(i, frame)
                }

                !starting[i] -> {
                    starting[i] = true
                    val params =
                        AdvertisingSetParameters
                            .Builder()
                            .setLegacyMode(true)
                            .setConnectable(false)
                            .setScannable(false)
                            .setInterval(AdvertisingSetParameters.INTERVAL_MEDIUM)
                            .setTxPowerLevel(AdvertisingSetParameters.TX_POWER_HIGH)
                            .build()
                    try {
                        advertiser.startAdvertisingSet(params, data(frame), null, null, null, callbacks[i], handler)
                    } catch (e: SecurityException) {
                        starting[i] = false
                        Log.w(TAG, "group: advertising permission", e)
                    }
                }
            }
        }
    }

    @SuppressLint("MissingPermission")
    override fun scan(
        prefixes: List<ByteArray>,
        batchDelayMs: Long,
        onFrames: (List<ByteArray>) -> Unit,
    ) {
        if (!available) return
        val scanner = adapter?.bluetoothLeScanner ?: return
        stopScan()
        val filters =
            prefixes.map { p ->
                ScanFilter
                    .Builder()
                    .setServiceData(parcelUuid, p, ByteArray(p.size) { MASK })
                    .build()
            }
        val batched = adapter.isOffloadedScanBatchingSupported
        val settings =
            ScanSettings
                .Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_POWER)
                .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
                .setReportDelay(if (batched) batchDelayMs else 0L)
                .build()
        val cb =
            object : ScanCallback() {
                override fun onScanResult(
                    callbackType: Int,
                    result: ScanResult,
                ) = deliver(listOf(result))

                override fun onBatchScanResults(results: MutableList<ScanResult>) = deliver(results)

                override fun onScanFailed(errorCode: Int) {
                    Log.w(TAG, "group: scan failed ($errorCode)")
                }

                private fun deliver(results: List<ScanResult>) {
                    val frames = results.mapNotNull { it.scanRecord?.getServiceData(parcelUuid) }
                    if (frames.isNotEmpty()) handler.post { onFrames(frames) }
                }
            }
        try {
            scanner.startScan(filters, settings, cb)
            scanCallback = cb
            Log.i(TAG, "group: scan of ${prefixes.size} tags, batches ${if (batched) "$batchDelayMs ms" else "off (not offloaded)"}")
        } catch (e: SecurityException) {
            Log.w(TAG, "group: scan permission", e)
        }
    }

    @SuppressLint("MissingPermission")
    private fun stopScan() {
        val cb = scanCallback ?: return
        scanCallback = null
        try {
            adapter?.bluetoothLeScanner?.stopScan(cb)
        } catch (e: SecurityException) {
            Log.w(TAG, "group: scan permission", e)
        }
    }

    @SuppressLint("MissingPermission")
    override fun stop() {
        stopScan()
        val advertiser = adapter?.bluetoothLeAdvertiser
        for (i in sets.indices) {
            try {
                if (sets[i] != null || starting[i]) advertiser?.stopAdvertisingSet(callbacks[i])
            } catch (e: SecurityException) {
                Log.w(TAG, "group: advertising permission", e)
            }
            sets[i] = null
            starting[i] = false
            pending[i] = null
        }
    }

    private companion object {
        const val TAG = "skimap"
        const val MASK: Byte = -1
    }
}
