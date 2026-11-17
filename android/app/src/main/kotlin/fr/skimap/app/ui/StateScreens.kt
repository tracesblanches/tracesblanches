package fr.skimap.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import fr.skimap.app.R
import fr.skimap.app.ui.theme.CommandTextStyle
import fr.skimap.app.ui.theme.GloveTarget

@Composable
fun LoadingScreen(modifier: Modifier = Modifier) {
    Surface(modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            CircularProgressIndicator(Modifier.size(72.dp), strokeWidth = 8.dp)
            Text(stringResource(R.string.loading), style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(16.dp))
        }
    }
}

@Composable
private fun MessageScreen(
    title: String,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Surface(modifier.fillMaxSize()) {
        Box(Modifier.windowInsetsPadding(WindowInsets.safeDrawing)) {
            Column(
                Modifier.verticalScroll(rememberScrollState()).padding(24.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
                content()
            }
        }
    }
}

/** No pack yet: download one from the areas of France (K2), or copy one with adb. */
@Composable
fun NoPackScreen(
    state: MapUiState.NoPack,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    onChooseDomain: () -> Unit = {},
) {
    MessageScreen(stringResource(R.string.no_pack_title), modifier) {
        Text(stringResource(R.string.no_pack_body), style = MaterialTheme.typography.bodyLarge)
        Button(onClick = onChooseDomain, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.choose_domain), style = MaterialTheme.typography.labelLarge)
        }
        Text(stringResource(R.string.no_pack_dir), style = MaterialTheme.typography.bodyMedium)
        SelectionContainer {
            Text("${state.packsDir}/<site>/manifest.json", style = CommandTextStyle)
        }
        SelectionContainer {
            Text("adb push data/out/<site>/pack/. ${state.packsDir}/<site>/", style = CommandTextStyle)
        }
        state.unreadable.forEach { e ->
            Text(
                stringResource(R.string.unreadable_pack, e.siteName) + " — " + e.error.orEmpty(),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
        OutlinedButton(onClick = onRetry, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.retry), style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** The chosen pack does not open: its error, and the other packs. */
@Composable
fun FailedScreen(
    state: MapUiState.Failed,
    onSelectPack: (String) -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
) {
    MessageScreen(stringResource(R.string.failed_title), modifier) {
        SelectionContainer { Text(PackTexts(LocalResources.current).problem(state.problem), style = CommandTextStyle) }
        state.packs.filter { it.error == null && it.dir != state.dir }.forEach { p ->
            OutlinedButton(onClick = { onSelectPack(p.dir) }, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
                Text(
                    stringResource(R.string.label_value, stringResource(R.string.choose_other), p.siteName),
                    style = MaterialTheme.typography.labelLarge,
                )
            }
        }
        Button(onClick = onRetry, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(stringResource(R.string.retry), style = MaterialTheme.typography.labelLarge)
        }
    }
}
