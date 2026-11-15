package fr.skimap.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import fr.skimap.app.R
import fr.skimap.app.ui.theme.GloveTarget

/** The two steps of the welcome: icon, title, text. */
private val STEPS =
    listOf(
        Triple(R.drawable.ic_day, R.string.welcome_morning_title, R.string.welcome_morning_body),
        Triple(R.drawable.ic_recenter, R.string.welcome_pocket_title, R.string.welcome_pocket_body),
    )

/**
 * First launch: the idea in two short steps (prepare the day in the morning, then the
 * phone in the pocket), skippable at any time, and one clear call: « Préparer ma
 * journée » (or choose an area when no pack is on the phone yet).
 */
@Composable
fun WelcomeScreen(
    hasPack: Boolean,
    onSkip: () -> Unit,
    onStart: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var step by rememberSaveable { mutableIntStateOf(0) }
    val last = step == STEPS.lastIndex
    val (icon, title, body) = STEPS[step]
    Surface(modifier.fillMaxSize().testTag(WelcomeTags.SCREEN)) {
        Box(Modifier.windowInsetsPadding(WindowInsets.safeDrawing).padding(24.dp), contentAlignment = Alignment.Center) {
            Column(
                Modifier.widthIn(max = 520.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(20.dp),
            ) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        stringResource(R.string.welcome_step, step + 1, STEPS.size),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    TextButton(onClick = onSkip, modifier = Modifier.heightIn(min = 48.dp).testTag(WelcomeTags.SKIP)) {
                        Text(stringResource(R.string.welcome_skip), style = MaterialTheme.typography.labelMedium)
                    }
                }
                Icon(
                    painterResource(icon),
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(56.dp),
                )
                Text(
                    stringResource(title),
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.semantics { heading() },
                )
                Text(stringResource(body), style = MaterialTheme.typography.bodyLarge)
                if (last) {
                    Button(
                        onClick = onStart,
                        modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(WelcomeTags.START),
                    ) {
                        Text(
                            stringResource(if (hasPack) R.string.day_prepare else R.string.welcome_choose_area),
                            style = MaterialTheme.typography.labelLarge,
                        )
                    }
                } else {
                    Button(
                        onClick = { step++ },
                        modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(WelcomeTags.NEXT),
                    ) {
                        Text(stringResource(R.string.welcome_next), style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        }
    }
}
