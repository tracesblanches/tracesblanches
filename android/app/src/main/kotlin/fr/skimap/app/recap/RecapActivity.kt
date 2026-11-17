package fr.skimap.app.recap

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.speech.tts.TextToSpeech
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.lifecycleScope
import fr.skimap.app.R
import fr.skimap.app.fog.Outings
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.core.OutingRecap
import fr.skimap.core.OutingSummary
import fr.skimap.core.outingDelete
import fr.skimap.core.outingDeleteAll
import fr.skimap.core.outingHistory
import fr.skimap.core.outingRecap
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.Locale

/** Test tags of the outings screen. */
object RecapTags {
    const val LIST = "recap_list"
    const val LISTEN = "recap_listen"
    const val DELETE = "recap_delete"
    const val DELETE_ALL = "recap_delete_all"
    const val CONFIRM = "recap_confirm"
}

/** What the outings screen shows. */
data class RecapUi(
    val history: List<OutingSummary> = emptyList(),
    val open: OutingRecap? = null,
)

/** Callbacks of [RecapScreen]. */
data class RecapActions(
    val open: (OutingSummary) -> Unit = {},
    val back: () -> Unit = {},
    val listen: (OutingRecap) -> Unit = {},
    val delete: (OutingSummary) -> Unit = {},
    val deleteAll: () -> Unit = {},
)

/**
 * The outings (A10): the history, the recap of each written and read aloud (distance,
 * time, estimated against actual, comparison with the previous outings, conditions,
 * climb by the barometer), and deleting one outing or all of them, with confirmation.
 */
class RecapActivity : ComponentActivity() {
    private var ui by mutableStateOf(RecapUi())
    private var tts: TextToSpeech? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val dir = intent.getStringExtra(EXTRA_DIR)
        val speak = intent.getBooleanExtra(EXTRA_SPEAK, false)
        lifecycleScope.launch {
            reload()
            if (dir != null) open(dir, speak)
        }
        setContent {
            SkimapTheme {
                Surface {
                    RecapScreen(
                        ui,
                        RecapActions(
                            open = { lifecycleScope.launch { open(it.dir, false) } },
                            back = { ui = ui.copy(open = null) },
                            listen = { say(it.spoken) },
                            delete = { s ->
                                lifecycleScope.launch {
                                    withContext(
                                        Dispatchers.IO,
                                    ) { runCatching { outingDelete(s.dir, Outings.plansRoot(applicationContext).path) } }
                                    ui = ui.copy(open = null)
                                    reload()
                                }
                            },
                            deleteAll = {
                                lifecycleScope.launch {
                                    // The outing in progress is kept.
                                    val keep =
                                        (Tracking.state.value as? TrackingState.Running)?.planDir ?: Tracking.activePlan(applicationContext)
                                    withContext(
                                        Dispatchers.IO,
                                    ) { runCatching { outingDeleteAll(Outings.plansRoot(applicationContext).path, keep) } }
                                    ui = ui.copy(open = null)
                                    reload()
                                }
                            },
                        ),
                    )
                }
            }
        }
    }

    private suspend fun reload() {
        val h = withContext(Dispatchers.IO) { outingHistory(Outings.plansRoot(applicationContext).path) }
        ui = ui.copy(history = h)
    }

    private suspend fun open(
        dir: String,
        speak: Boolean,
    ) {
        val r =
            withContext(Dispatchers.Default) {
                runCatching {
                    outingRecap(
                        dir,
                        Outings.plansRoot(applicationContext).path,
                        Outings.workDir(applicationContext).path,
                    )
                }.getOrNull()
            } ?: return
        ui = ui.copy(open = r)
        if (speak) say(r.spoken)
    }

    private fun say(text: String) {
        val t = tts
        if (t != null) {
            t.speak(text, TextToSpeech.QUEUE_FLUSH, null, "recap")
            return
        }
        var created: TextToSpeech? = null
        created =
            TextToSpeech(applicationContext) { status ->
                if (status == TextToSpeech.SUCCESS) {
                    created?.language = Locale.FRANCE
                    created?.speak(text, TextToSpeech.QUEUE_FLUSH, null, "recap")
                }
            }
        tts = created
    }

    override fun onDestroy() {
        tts?.shutdown()
        super.onDestroy()
    }

    companion object {
        const val EXTRA_DIR = "dir"
        const val EXTRA_SPEAK = "speak"

        fun intent(
            c: Context,
            dir: String? = null,
            speak: Boolean = false,
        ): Intent =
            Intent(c, RecapActivity::class.java)
                .putExtra(EXTRA_DIR, dir)
                .putExtra(EXTRA_SPEAK, speak)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
}

/** The outings screen, stateless (captured in the JVM tests). */
@Composable
fun RecapScreen(
    ui: RecapUi,
    actions: RecapActions,
) {
    var confirm by rememberSaveable { mutableStateOf<String?>(null) }
    confirm?.let { what ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            text = { Text(stringResource(if (what == ALL) R.string.recap_delete_all_confirm else R.string.recap_delete_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirm = null
                    if (what == ALL) actions.deleteAll() else ui.open?.let { actions.delete(it.summary) }
                }, modifier = Modifier.testTag(RecapTags.CONFIRM)) { Text(stringResource(R.string.recap_delete_yes)) }
            },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text(stringResource(R.string.recap_cancel)) } },
        )
    }
    LazyColumn(
        Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).testTag(RecapTags.LIST),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val r = ui.open
        if (r != null) {
            item {
                Text(r.summary.title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
                Text(r.summary.`when`, style = MaterialTheme.typography.bodyMedium)
            }
            item { Lines(r.figures) }
            item { Lines(r.comparisons) }
            item { Lines(r.conditions) }
            item {
                Button(
                    onClick = { actions.listen(r) },
                    modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(RecapTags.LISTEN),
                ) {
                    Text(stringResource(R.string.recap_listen))
                }
            }
            item {
                OutlinedButton(onClick = actions.back, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
                    Text(stringResource(R.string.recap_back))
                }
            }
            item {
                TextButton(
                    onClick = { confirm = ONE },
                    modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(RecapTags.DELETE),
                ) {
                    Text(stringResource(R.string.recap_delete), color = MaterialTheme.colorScheme.error)
                }
            }
            return@LazyColumn
        }
        item {
            Text(
                stringResource(R.string.recap_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.semantics { heading() },
            )
        }
        if (ui.history.isEmpty()) {
            item { Text(stringResource(R.string.recap_none), style = MaterialTheme.typography.bodyLarge) }
        }
        items(ui.history, key = { it.dir }) { s -> OutingRow(s) { actions.open(s) } }
        if (ui.history.isNotEmpty()) {
            item {
                TextButton(
                    onClick = { confirm = ALL },
                    modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(RecapTags.DELETE_ALL),
                ) {
                    Text(stringResource(R.string.recap_delete_all), color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun Lines(lines: List<String>) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            lines.forEach { Text(it, style = MaterialTheme.typography.bodyLarge) }
        }
    }
}

@Composable
private fun OutingRow(
    s: OutingSummary,
    onClick: () -> Unit,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).clickable(onClick = onClick),
    ) {
        Column(Modifier.padding(12.dp)) {
            Text(s.title, style = MaterialTheme.typography.titleMedium)
            val source = if (s.kind == "strava") " · Strava" else ""
            Text(
                "${s.`when`} · " + "%.1f km".format(LocalConfiguration.current.locales[0], s.distanceKm) + source,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

private const val ONE = "one"
private const val ALL = "all"
