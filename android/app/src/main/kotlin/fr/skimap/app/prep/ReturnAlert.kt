package fr.skimap.app.prep

import android.content.Context
import android.util.Log
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.core.content.edit
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import fr.skimap.app.R
import fr.skimap.app.day.DayStore
import fr.skimap.app.sync.PackSync
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.DayPlanView
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.time.Instant

/** State of the server alert of an outing. */
enum class AlertState { OFF, PENDING, ACTIVE, UNAVAILABLE, FAILED, RETURNED }

/** An answer of the server: HTTP status and body. */
data class HttpAnswer(
    val status: Int,
    val body: String,
)

/** The server's trips API (S2, `crates/server/docs/api.md`): one POST at a time. */
fun interface TripsApi {
    @Throws(IOException::class)
    fun post(
        path: String,
        json: String,
        token: String?,
    ): HttpAnswer
}

/** [TripsApi] over HttpURLConnection, on the server of the packs. */
class HttpTripsApi(
    private val base: String,
) : TripsApi {
    override fun post(
        path: String,
        json: String,
        token: String?,
    ): HttpAnswer {
        val c = URL(base.trimEnd('/') + path).openConnection() as HttpURLConnection
        try {
            c.requestMethod = "POST"
            c.connectTimeout = TIMEOUT_MS
            c.readTimeout = TIMEOUT_MS
            c.doOutput = true
            c.setRequestProperty("Content-Type", "application/json")
            token?.let { c.setRequestProperty("Authorization", "Bearer $it") }
            c.outputStream.use { it.write(json.toByteArray()) }
            val code = c.responseCode
            val stream = if (code >= HttpURLConnection.HTTP_BAD_REQUEST) c.errorStream else c.inputStream
            return HttpAnswer(code, stream?.bufferedReader()?.use { it.readText() }.orEmpty())
        } finally {
            c.disconnect()
        }
    }

    private companion object {
        const val TIMEOUT_MS = 15_000
    }
}

/**
 * The optional non-return alert by the server (S2 #60), off by default: when on, the plan
 * is deposited on the server at the start (a WorkManager job, as soon as the network allows)
 * and the return is confirmed at the end; the server e-mails the contacts if the return is
 * not confirmed after the deadline and its grace. The SMS to the contact (K10) stays the
 * main way; a 503 (service not configured) is shown as « service non disponible ».
 */
@Suppress("TooManyFunctions")
object ReturnAlert {
    private const val PREFS = "skimap_return_alert"
    private const val KEY_ON = "on"
    private const val KEY_EMAILS = "emails"
    const val TRIP_FILE = "server_trip.json"
    private const val GRACE_MIN = 30
    private const val HTTP_UNAVAILABLE = 503

    private fun prefs(c: Context) = c.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun enabled(c: Context) = prefs(c).getBoolean(KEY_ON, false)

    fun emails(c: Context): List<String> =
        prefs(c)
            .getString(KEY_EMAILS, "")
            .orEmpty()
            .split(',', ';', ' ')
            .map { it.trim() }
            .filter { '@' in it }
            .take(MAX_CONTACTS)

    fun setEnabled(
        c: Context,
        on: Boolean,
    ) = prefs(c).edit { putBoolean(KEY_ON, on) }

    fun setEmails(
        c: Context,
        emails: String,
    ) = prefs(c).edit { putString(KEY_EMAILS, emails) }

    /** State of the outing in [planDir]. */
    @Suppress("ReturnCount")
    fun state(planDir: String?): Pair<AlertState, String?> {
        val f = planDir?.let { File(it, TRIP_FILE) }?.takeIf { it.isFile } ?: return AlertState.OFF to null
        val o = runCatching { JSONObject(f.readText()) }.getOrNull() ?: return AlertState.OFF to null
        return runCatching { AlertState.valueOf(o.optString("state")) }.getOrDefault(AlertState.OFF) to
            o.optString("detail").ifEmpty { null }
    }

    private fun write(
        planDir: String,
        o: JSONObject,
    ) = File(planDir, TRIP_FILE).writeText(o.toString())

    /** The body of `POST /v1/trips` for a plan. */
    fun tripJson(
        plan: DayPlanView,
        contacts: List<String>,
        name: String?,
        phone: String?,
    ): String {
        val route = JSONArray()
        val step = maxOf(2, (plan.line.size / 2 / MAX_ROUTE_POINTS) * 2)
        var i = 0
        while (i + 1 < plan.line.size) {
            route.put(JSONArray().put(plan.line[i]).put(plan.line[i + 1]))
            i += step
        }
        val deadline = plan.limitMs ?: plan.expectedBackMs
        return JSONObject()
            .put("skier_name", name ?: "")
            .put("skier_phone", phone ?: "")
            .put("contacts", JSONArray(contacts))
            .put("deadline", Instant.ofEpochMilli(deadline).toString())
            .put("grace_minutes", GRACE_MIN)
            .put("site", plan.siteId)
            .put("plan", plan.sms.take(MAX_PLAN_CHARS))
            .put("route", route)
            .toString()
    }

    /** At the start of an outing: deposit its plan when the option is on. */
    fun deposit(
        context: Context,
        plan: DayPlanView,
    ) {
        if (!enabled(context) || emails(context).isEmpty()) return
        val contact = DayStore(context).contact()
        val body = tripJson(plan, emails(context), contact?.name, null)
        write(plan.planDir, JSONObject().put("state", AlertState.PENDING.name).put("body", body))
        enqueue(context, plan.planDir, DEPOSIT)
    }

    /** At the end of an outing: confirm the return if a plan was deposited. */
    fun checkIn(
        context: Context,
        planDir: String,
    ) {
        val (s, _) = state(planDir)
        if (s == AlertState.ACTIVE || s == AlertState.PENDING) enqueue(context, planDir, CHECKIN)
    }

    private fun enqueue(
        context: Context,
        planDir: String,
        what: String,
    ) {
        val req =
            OneTimeWorkRequestBuilder<ReturnAlertWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setInputData(workDataOf(KEY_DIR to planDir, KEY_WHAT to what))
                .build()
        WorkManager.getInstance(context).enqueueUniqueWork("return-alert-$what", ExistingWorkPolicy.REPLACE, req)
    }

    /** Runs a deposit or a check-in for the outing in [planDir]; whether to retry. */
    @Suppress("ReturnCount", "NestedBlockDepth")
    fun run(
        api: TripsApi,
        planDir: String,
        what: String,
    ): Boolean {
        val f = File(planDir, TRIP_FILE)
        val o = runCatching { JSONObject(f.readText()) }.getOrNull() ?: return false
        return try {
            if (what == DEPOSIT) {
                // A check-in queued before the deposit went: nothing to deposit any more.
                if (o.optString("state") != AlertState.PENDING.name) return false
                val a = api.post("/v1/trips", o.getString("body"), null)
                when {
                    a.status == HTTP_CREATED -> {
                        val r = JSONObject(a.body)
                        o
                            .put(
                                "state",
                                AlertState.ACTIVE.name,
                            ).put("id", r.getString("id"))
                            .put("token", r.getString("token"))
                            .remove("body")
                    }

                    a.status == HTTP_UNAVAILABLE -> {
                        o.put("state", AlertState.UNAVAILABLE.name)
                    }

                    else -> {
                        o.put("state", AlertState.FAILED.name).put("detail", "HTTP ${a.status} ${a.body.take(DETAIL_CHARS)}")
                    }
                }
            } else {
                if (o.optString("state") == AlertState.PENDING.name) {
                    // Back before the plan could leave: nothing on the server.
                    o.put("state", AlertState.RETURNED.name).remove("body")
                } else {
                    val a = api.post("/v1/trips/${o.getString("id")}/checkin", "{}", o.getString("token"))
                    if (a.status in HTTP_OK..HTTP_LAST_OK) {
                        o.put("state", AlertState.RETURNED.name)
                    } else if (a.status == HTTP_UNAVAILABLE) {
                        o.put("detail", "HTTP 503")
                        write(planDir, o)
                        return true
                    } else {
                        o.put("detail", "HTTP ${a.status}")
                    }
                }
            }
            write(planDir, o)
            false
        } catch (e: IOException) {
            Log.w("skimap", "return alert $what", e)
            true
        }
    }

    const val KEY_DIR = "dir"
    const val KEY_WHAT = "what"
    const val DEPOSIT = "deposit"
    const val CHECKIN = "checkin"
    private const val MAX_CONTACTS = 3
    private const val MAX_ROUTE_POINTS = 500
    private const val MAX_PLAN_CHARS = 2000
    private const val DETAIL_CHARS = 120
    private const val HTTP_CREATED = 201
    private const val HTTP_OK = 200
    private const val HTTP_LAST_OK = 299
}

/** The WorkManager job of [ReturnAlert]: needs the network, retried with backoff on network errors. */
class ReturnAlertWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    @Suppress("ReturnCount")
    override suspend fun doWork(): Result {
        val dir = inputData.getString(ReturnAlert.KEY_DIR) ?: return Result.failure()
        val what = inputData.getString(ReturnAlert.KEY_WHAT) ?: return Result.failure()
        val retry = ReturnAlert.run(HttpTripsApi(PackSync.serverUrl(applicationContext)), dir, what)
        return if (retry) Result.retry() else Result.success()
    }
}

/** Test tag of the server alert card. */
const val RETURN_ALERT_CARD = "return_alert_card"

/** The option on the plan screen: off by default, the contacts' e-mails, the state. */
@Composable
fun ReturnAlertCard(
    planDir: String?,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    var on by remember { mutableStateOf(ReturnAlert.enabled(context)) }
    var emails by remember { mutableStateOf(ReturnAlert.emails(context).joinToString(", ")) }
    val (state, detail) = remember(planDir, on) { ReturnAlert.state(planDir) }
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = modifier.fillMaxWidth().testTag(RETURN_ALERT_CARD),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = GloveTarget).toggleable(on, role = Role.Switch) {
                    on = it
                    ReturnAlert.setEnabled(context, it)
                },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(stringResource(R.string.alert_title), style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f))
                Switch(checked = on, onCheckedChange = null)
            }
            Text(stringResource(R.string.alert_explain), style = MaterialTheme.typography.bodySmall)
            if (on) {
                OutlinedTextField(
                    value = emails,
                    onValueChange = {
                        emails = it
                        ReturnAlert.setEmails(context, it)
                    },
                    label = { Text(stringResource(R.string.alert_emails)) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            val s =
                when (state) {
                    AlertState.OFF -> stringResource(R.string.alert_status_off)
                    AlertState.PENDING -> stringResource(R.string.alert_status_pending)
                    AlertState.ACTIVE -> stringResource(R.string.alert_status_active)
                    AlertState.UNAVAILABLE -> stringResource(R.string.alert_status_unavailable)
                    AlertState.FAILED -> stringResource(R.string.alert_status_failed, detail.orEmpty())
                    AlertState.RETURNED -> stringResource(R.string.alert_status_returned)
                }
            Text(s, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/** On the plan screen, only when the plan went to the server: its state, one line. */
@Composable
fun ReturnAlertStatus(planDir: String?) {
    val (state, detail) = remember(planDir) { ReturnAlert.state(planDir) }
    if (state == AlertState.OFF) return
    val s =
        when (state) {
            AlertState.PENDING -> stringResource(R.string.alert_status_pending)
            AlertState.ACTIVE -> stringResource(R.string.alert_status_active)
            AlertState.UNAVAILABLE -> stringResource(R.string.alert_status_unavailable)
            AlertState.FAILED -> stringResource(R.string.alert_status_failed, detail.orEmpty())
            else -> stringResource(R.string.alert_status_returned)
        }
    Text(
        "${stringResource(R.string.settings_alert)} : $s",
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.testTag(RETURN_ALERT_CARD),
    )
}
