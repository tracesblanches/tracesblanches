package fr.skimap.app.voicecmd

import android.content.Context
import android.util.Log
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import fr.skimap.voicecmd.ModelInstaller
import fr.skimap.voicecmd.ModelStore
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * The speech model of the voice commands, downloaded on its own with the packs, never
 * asked (« on automatise ») : Wi-Fi, phone charging, storage not low (643 MiB). A run cut
 * by WorkManager's 10 min limit or the network resumes where it stopped (HTTP Range).
 */
class VoiceModelWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val store = store(applicationContext)
        if (store.installed) return Result.success()
        return try {
            ModelInstaller(store).install()
            Log.i(TAG, "voice model installed: ${store.bytesOnDisk} bytes")
            Result.success()
        } catch (e: IOException) {
            Log.w(TAG, "voice model download", e)
            Result.retry()
        }
    }

    companion object {
        private const val TAG = "skimap"
        private const val NAME = "voice-model"
        private const val BACKOFF_MINUTES = 30L

        fun store(context: Context) = ModelStore(File(context.filesDir, "voicecmd"))

        /** Queued with the daily pack update (no disk access here: the worker checks the model). */
        fun schedule(context: Context) {
            val request =
                OneTimeWorkRequestBuilder<VoiceModelWorker>()
                    .setConstraints(
                        Constraints
                            .Builder()
                            .setRequiredNetworkType(NetworkType.UNMETERED)
                            .setRequiresCharging(true)
                            .setRequiresStorageNotLow(true)
                            .build(),
                    ).setBackoffCriteria(BackoffPolicy.EXPONENTIAL, BACKOFF_MINUTES, TimeUnit.MINUTES)
                    .build()
            WorkManager.getInstance(context).enqueueUniqueWork(NAME, ExistingWorkPolicy.KEEP, request)
        }
    }
}
