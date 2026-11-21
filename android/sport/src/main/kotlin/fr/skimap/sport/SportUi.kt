package fr.skimap.sport

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** Test tags of the share. */
object SportTags {
    const val SHARE_FIT = "sport_share_fit"
    const val SHARE_GPX = "sport_share_gpx"
}

/** Glove-size targets, as the app's (GloveTarget). */
private val Target = 72.dp

/**
 * « Partager la sortie » on the recap of a finished outing: the FIT file (the sport kept:
 * Strava, Garmin Connect) or the GPX (every app); Health Connect at the same time.
 */
@Composable
fun ShareOuting(
    onShare: (ShareFormat) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(
            onClick = { onShare(ShareFormat.FIT) },
            modifier = Modifier.fillMaxWidth().heightIn(min = Target).testTag(SportTags.SHARE_FIT),
        ) { Text(stringResource(R.string.sport_share)) }
        OutlinedButton(
            onClick = { onShare(ShareFormat.GPX) },
            modifier = Modifier.fillMaxWidth().heightIn(min = Target).testTag(SportTags.SHARE_GPX),
        ) { Text(stringResource(R.string.sport_share_gpx)) }
        Text(stringResource(R.string.sport_share_hint), style = MaterialTheme.typography.bodySmall)
    }
}

/**
 * The page Health Connect opens to explain what the app does with the data
 * (`androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE`, and `VIEW_PERMISSION_USAGE` on
 * Android 14+). The app subclasses it for its theme ([Themed]) and declares it.
 */
open class HealthRationaleActivity : ComponentActivity() {
    /** The app's theme around the page; Material's by default. */
    @Composable
    protected open fun Themed(content: @Composable () -> Unit) {
        MaterialTheme(colorScheme = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme(), content = content)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent { Themed { Surface { HealthRationale(onClose = ::finish) } } }
    }

    companion object {
        /** Health Connect asks the app to explain its use of the data. */
        const val ACTION_RATIONALE = "androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE"
    }
}

/** What is written to Health Connect and what is read, in plain words. */
@Composable
fun HealthRationale(onClose: () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            stringResource(R.string.sport_rationale_title),
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.semantics { heading() },
        )
        Text(stringResource(R.string.sport_rationale_text), style = MaterialTheme.typography.bodyLarge)
        Button(onClick = onClose, modifier = Modifier.fillMaxWidth().heightIn(min = Target)) { Text(stringResource(R.string.sport_close)) }
    }
}
