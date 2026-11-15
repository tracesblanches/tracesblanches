package fr.skimap.sport

import android.app.Application
import android.net.Uri
import android.util.Log
import androidx.health.connect.client.HealthConnectClient
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.IOException
import java.security.SecureRandom

/** Health Connect seen from the screen. */
enum class HealthUi { UNAVAILABLE, NEEDS_UPDATE, NOT_ALLOWED, READY }

/** Strava seen from the screen. */
sealed interface StravaUi {
    data object Checking : StravaUi

    /** skimap-server has no Strava application yet (`configured: false`). */
    data object NotConfigured : StravaUi

    data object Unreachable : StravaUi

    data object Disconnected : StravaUi

    data object Connecting : StravaUi

    data class Connected(
        val athlete: String?,
    ) : StravaUi
}

/** The screen « Envoyer mes sorties ». */
data class SportUi(
    val health: HealthUi = HealthUi.UNAVAILABLE,
    val strava: StravaUi = StravaUi.Checking,
    val autoSend: Boolean = false,
    val queue: List<Queued> = emptyList(),
    /** Last error to show (Strava refused, wrong state…), already in words. */
    val error: String? = null,
)

/**
 * The state of « Envoyer mes sorties »: Health Connect, Strava (sign-in through the
 * browser and skimap-server, sign-out with the revocation of the token), the automatic
 * sending and the queue.
 */
class SportViewModel(
    app: Application,
) : AndroidViewModel(app) {
    private val store = SportStore(app)
    private val mutable = MutableStateFlow(SportUi())
    val state: StateFlow<SportUi> = mutable.asStateFlow()

    init {
        viewModelScope.launch { SportStore.revision.collect { refreshLocal() } }
        refresh()
    }

    /** Re-reads everything (also asks skimap-server whether Strava is set up). */
    fun refresh() {
        viewModelScope.launch {
            refreshLocal()
            val s =
                withContext(Dispatchers.IO) {
                    val t = store.tokens()
                    if (t != null) {
                        StravaUi.Connected(t.athlete)
                    } else {
                        try {
                            if (Sport.strava(store).config().configured) StravaUi.Disconnected else StravaUi.NotConfigured
                        } catch (e: IOException) {
                            Log.w(Sport.TAG, "sport: strava config", e)
                            StravaUi.Unreachable
                        }
                    }
                }
            mutable.update { if (it.strava == StravaUi.Connecting) it else it.copy(strava = s) }
        }
    }

    private suspend fun refreshLocal() {
        val (h, q) =
            withContext(Dispatchers.IO) {
                val sender = HealthConnectSender(getApplication())
                val h =
                    when (sender.status()) {
                        HealthConnectClient.SDK_AVAILABLE -> if (sender.granted()) HealthUi.READY else HealthUi.NOT_ALLOWED
                        HealthConnectClient.SDK_UNAVAILABLE_PROVIDER_UPDATE_REQUIRED -> HealthUi.NEEDS_UPDATE
                        else -> HealthUi.UNAVAILABLE
                    }
                h to store.queue()
            }
        val tokens = store.tokens()
        mutable.update {
            it.copy(
                health = h,
                autoSend = store.autoSend(),
                queue = q,
                strava =
                    if (tokens !=
                        null
                    ) {
                        StravaUi.Connected(tokens.athlete)
                    } else {
                        it.strava.takeUnless { s -> s is StravaUi.Connected }
                            ?: StravaUi.Disconnected
                    },
            )
        }
    }

    fun setAutoSend(on: Boolean) = store.setAutoSend(on)

    /** Health Connect answered the permission request. */
    fun healthAnswered(granted: Set<String>) {
        if (granted.containsAll(HealthConnectSender.PERMISSIONS)) Sport.resume(getApplication())
        viewModelScope.launch { refreshLocal() }
    }

    /** The authorisation page of Strava to open in the browser; `null` if Strava cannot be reached. */
    suspend fun authorizeUrl(): String? =
        withContext(Dispatchers.IO) {
            try {
                val client = Sport.strava(store)
                val c = client.config()
                if (!c.configured) {
                    mutable.update { it.copy(strava = StravaUi.NotConfigured) }
                    return@withContext null
                }
                val state = randomState()
                store.setOauthState(state)
                client.authorizeUrl(c, state)
            } catch (e: IOException) {
                Log.w(Sport.TAG, "sport: strava config", e)
                mutable.update { it.copy(strava = StravaUi.Unreachable) }
                null
            }
        }

    /**
     * `skimap://strava/callback?code=…&scope=…&state=…` (skimap-server redirects there):
     * the state is checked, the code exchanged for the tokens through the server.
     */
    fun callback(uri: Uri) {
        val expected = store.oauthState()
        store.setOauthState(null)
        val code = uri.getQueryParameter("code")
        val scope = uri.getQueryParameter("scope").orEmpty()
        val texts = getApplication<Application>().resources
        when {
            expected == null || uri.getQueryParameter("state") != expected -> {
                mutable.update { it.copy(error = texts.getString(R.string.sport_strava_state)) }
                return
            }

            uri.getQueryParameter("error") != null || code == null || "activity:write" !in scope.split(',') -> {
                mutable.update { it.copy(error = texts.getString(R.string.sport_strava_denied)) }
                return
            }
        }
        mutable.update { it.copy(strava = StravaUi.Connecting, error = null) }
        viewModelScope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    runCatching { Sport.strava(store).exchange(code!!) }
                }
            result
                .onSuccess {
                    store.setTokens(it)
                    Sport.resume(getApplication())
                    mutable.update { s -> s.copy(strava = StravaUi.Connected(it.athlete)) }
                }.onFailure { e ->
                    Log.w(Sport.TAG, "sport: strava token", e)
                    mutable.update { s ->
                        s.copy(strava = StravaUi.Disconnected, error = texts.getString(R.string.sport_strava_error, e.message.orEmpty()))
                    }
                }
        }
    }

    /**
     * Disconnects Strava: the token is revoked through skimap-server (Strava forgets the
     * authorisation) and removed from the phone, even when the revocation cannot be made
     * (no network): it is then only removed here.
     */
    fun disconnect() {
        val t = store.tokens() ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching { Sport.strava(store).revoke(t) }.onFailure { Log.w(Sport.TAG, "sport: strava revoke", it) }
                store.setTokens(null)
            }
            mutable.update { it.copy(strava = StravaUi.Disconnected) }
            refresh()
        }
    }

    fun clearError() = mutable.update { it.copy(error = null) }

    private fun randomState(): String {
        val b = ByteArray(STATE_BYTES)
        SecureRandom().nextBytes(b)
        return b.joinToString("") { "%02x".format(it) }
    }

    private companion object {
        const val STATE_BYTES = 16
    }
}
