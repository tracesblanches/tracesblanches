package fr.skimap.app.day

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import fr.skimap.app.MainActivity
import fr.skimap.app.R
import fr.skimap.app.guidance.SkiTrackingService
import fr.skimap.app.recap.OutingFiles
import fr.skimap.app.ui.CoreErrors
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.core.DayPlanView
import fr.skimap.sport.ShareFlow
import fr.skimap.sport.ShareFormat
import fr.skimap.tracking.ReturnReminder
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.time.LocalDate

/**
 * The morning and the plan of the day (J5, K9, K10): the loops to choose, « Valider la
 * journée » (one gesture: the plan, the location service, the return reminder, the SMS to
 * the contact pre-filled), then the plan during the outing.
 */
class DayActivity : ComponentActivity() {
    private val vm: DayViewModel by viewModels {
        viewModelFactory {
            initializer {
                val ctx = applicationContext
                DayViewModel(
                    backend = RustDayBackend,
                    store = DayStore(ctx),
                    packsDir = { MainActivity.packsDir(ctx) },
                    workDir = { File(ctx.filesDir, "packview") },
                    preferredSite = intent.getStringExtra(EXTRA_SITE),
                    today = LocalDate.now(),
                    errorText = { CoreErrors.text(it, ctx.resources) },
                )
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val active = (Tracking.state.value as? TrackingState.Running)?.planDir ?: Tracking.activePlan(this)
        active?.let(vm::showPlan)
        val sharing = ShareFlow(this)
        setContent { SkimapTheme { Surface { DayRoot(vm) { tracking, format -> shareOuting(tracking, format, sharing) } } } }
    }

    /** The finished outing's file in the share menu, and Health Connect (#63). */
    private fun shareOuting(
        tracking: TrackingState,
        format: ShareFormat,
        sharing: ShareFlow,
    ) {
        val ended = tracking as? TrackingState.Ended ?: return
        sharing.share(
            format,
            { heart -> OutingFiles.export(applicationContext, ended.planDir, heart, ended.recap?.text) },
        ) { OutingFiles.tell(this, it) }
    }

    companion object {
        const val EXTRA_SITE = "site"

        /** Asked at « Valider la journée »: the precise position, and the notification (Android 13+). */
        fun permissionsForStart(): Array<String> =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.POST_NOTIFICATIONS)
            } else {
                arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
            }

        /** The SMS app opened on the contact (or none) with the plan: the user sends it. */
        fun sendPlan(
            context: Context,
            plan: DayPlanView,
            contact: Contact?,
        ) {
            val i = ReturnReminder.smsIntent(contact?.number.orEmpty(), plan.sms)
            try {
                context.startActivity(i)
            } catch (_: ActivityNotFoundException) {
                context.startActivity(
                    Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, plan.sms), null),
                )
            }
        }

        /** The dialer with 112: the app never calls by itself. */
        fun dial112(context: Context) {
            context.startActivity(Intent(Intent.ACTION_DIAL, "tel:112".toUri()))
        }

        /** Starts the outing: the location service, the reminder before the return time. */
        fun startOuting(
            context: Context,
            plan: DayPlanView,
            contact: Contact?,
        ) {
            Tracking.start(context, SkiTrackingService::class.java, plan.planDir)
            // Optional non-return alert by the server (S2): deposited when the option is on.
            fr.skimap.app.prep.ReturnAlert
                .deposit(context, plan)
            val at = plan.expectedBackMs - REMINDER_BEFORE_MS
            if (at > System.currentTimeMillis()) {
                val who = contact?.name?.ifBlank { contact.number } ?: "votre contact"
                ReturnReminder.schedule(
                    context,
                    at,
                    context.getString(R.string.reminder_title, plan.expectedBack),
                    context.getString(R.string.reminder_text, who, plan.limit ?: plan.expectedBack),
                    contact?.number,
                    context.getString(R.string.sms_back_home),
                )
            }
        }

        private const val REMINDER_BEFORE_MS = 15 * 60_000L
    }
}

@Composable
private fun DayRoot(
    vm: DayViewModel,
    shareOuting: (TrackingState, ShareFormat) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val resources = androidx.compose.ui.platform.LocalResources.current
    val state by vm.state.collectAsStateWithLifecycle()
    val plan by vm.validated.collectAsStateWithLifecycle()
    val tracking by Tracking.state.collectAsStateWithLifecycle()
    var validating by remember { mutableStateOf(false) }
    var morning by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val contact = (state as? DayUi.Ready)?.contact
    val validate = {
        validating = true
        scope.launch {
            vm
                .validate()
                .onSuccess { p ->
                    DayActivity.startOuting(context, p, contact)
                    morning = false
                    DayActivity.sendPlan(context, p, contact)
                }.onFailure { Toast.makeText(context, CoreErrors.text(it, resources), Toast.LENGTH_LONG).show() }
            validating = false
        }
    }
    val noPermission = stringResource(R.string.day_permission)
    val ask =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { granted ->
            if (granted[Manifest.permission.ACCESS_FINE_LOCATION] == true) {
                validate()
            } else {
                Toast.makeText(context, noPermission, Toast.LENGTH_LONG).show()
            }
        }
    LaunchedEffect(tracking) {
        // The outing ended: what the group learned is taken into account at once.
        if (tracking is TrackingState.Ended) {
            (tracking as TrackingState.Ended)
                .planDir
                .takeIf {
                    it.isNotEmpty()
                }?.let { DayStore(context).learnFrom(it) }
        }
    }
    val p = plan
    if (p != null && !morning) {
        PlanScreen(
            plan = p,
            tracking = tracking,
            actions =
                PlanActions(
                    send = { DayActivity.sendPlan(context, p, contact) },
                    emergency = { DayActivity.dial112(context) },
                    bringBack = { Tracking.command(context, SkiTrackingService::class.java, Tracking.ACTION_BACK) },
                    stop = { Tracking.command(context, SkiTrackingService::class.java, Tracking.ACTION_STOP) },
                    resume = { DayActivity.startOuting(context, p, contact) },
                    newDay = { morning = true },
                    answer = { a ->
                        Tracking.command(
                            context,
                            SkiTrackingService::class.java,
                            if (a) Tracking.ACTION_ACCEPT else Tracking.ACTION_DECLINE,
                        )
                    },
                    refuseSwitch = { Tracking.command(context, SkiTrackingService::class.java, Tracking.ACTION_REFUSE_SWITCH) },
                    shareOuting = { shareOuting(tracking, it) },
                ),
        )
        return
    }
    when (val s = state) {
        DayUi.Loading -> {
            DayMessage(null)
        }

        DayUi.Unavailable -> {
            DayMessage(stringResource(R.string.day_no_pack))
        }

        is DayUi.Ready -> {
            DayScreen(
                state = s,
                validating = validating,
                actions =
                    DayActions(
                        setInputs = vm::setInputs,
                        choose = vm::choose,
                        selectGroup = vm::selectGroup,
                        addGroup = vm::addGroup,
                        setContact = vm::setContact,
                        validate = {
                            val missing =
                                DayActivity.permissionsForStart().any {
                                    ContextCompat.checkSelfPermission(context, it) != PackageManager.PERMISSION_GRANTED
                                }
                            if (missing) ask.launch(DayActivity.permissionsForStart()) else validate()
                        },
                    ),
            )
        }
    }
}
