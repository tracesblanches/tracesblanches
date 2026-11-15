package fr.skimap.app.ui.domains

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import fr.skimap.app.R
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.Domain
import fr.skimap.core.PackStatus
import java.util.Locale
import kotlin.math.cos
import kotlin.math.min

/**
 * Area picker: the nordic areas of France on a map (dots; installed packs as squares),
 * search by name or commune, nearest areas, and the state of each pack. Only installed
 * packs open today; download is K2.
 */
@Composable
fun DomainPicker(
    state: DomainsState,
    onQuery: (String) -> Unit,
    onNearest: () -> Unit,
    onPick: (String) -> Unit,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.domains_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.weight(1f).semantics { heading() },
            )
            TextButton(onClick = onClose, modifier = Modifier.heightIn(min = GloveTarget)) {
                Text(stringResource(R.string.close), style = MaterialTheme.typography.labelLarge)
            }
        }
        OutlinedTextField(
            value = state.query,
            onValueChange = onQuery,
            label = { Text(stringResource(R.string.domains_search)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(DomainTags.SEARCH),
        )
        OutlinedButton(onClick = onNearest, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
            Text(
                stringResource(if (state.locating) R.string.domains_locating else R.string.domains_nearest),
                style = MaterialTheme.typography.labelLarge,
            )
        }
        if (state.noPosition) {
            Text(stringResource(R.string.domains_no_position), color = MaterialTheme.colorScheme.error)
        }
        FranceMap(state, Modifier.fillMaxWidth().height(220.dp))
        Text(
            pluralStringResource(R.plurals.domains_count, state.results.size, state.results.size, state.all.size),
            style = MaterialTheme.typography.bodySmall,
        )
        LazyColumn(Modifier.weight(1f).testTag(DomainTags.LIST)) {
            items(state.results, key = { it.id }) { d ->
                DomainRow(d, state.distances[d.id], onPick)
                HorizontalDivider()
            }
        }
    }
}

@Composable
private fun DomainRow(
    d: Domain,
    km: Double?,
    onPick: (String) -> Unit,
) {
    val dir = d.packDir
    val status =
        stringResource(
            when (d.status) {
                PackStatus.INSTALLED -> R.string.domain_installed
                PackStatus.AVAILABLE -> R.string.domain_available
                PackStatus.NOT_BUILT -> R.string.domain_not_built
            },
        )
    val details =
        buildList {
            if (km != null) add(stringResource(R.string.domain_distance, String.format(Locale.FRENCH, "%.0f", km)))
            add(d.place)
            add(stringResource(R.string.domain_km, String.format(Locale.FRENCH, "%.0f", d.kmNordic)))
        }.joinToString(" · ")
    val row =
        Modifier
            .fillMaxWidth()
            .heightIn(min = GloveTarget)
            .let { if (dir != null) it.clickable(role = Role.Button) { onPick(dir) } else it }
            .padding(vertical = 8.dp)
    Row(row, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        StatusMark(d.status, Modifier.size(20.dp))
        Column(Modifier.weight(1f)) {
            Text(d.name, style = MaterialTheme.typography.titleMedium)
            Text(details, style = MaterialTheme.typography.bodyMedium)
            Text(
                status,
                style = MaterialTheme.typography.bodyMedium,
                color = if (dir != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Square: installed; ring: not on the phone. Shape and word, never colour alone. */
@Composable
private fun StatusMark(
    status: PackStatus,
    modifier: Modifier = Modifier,
) {
    val primary = MaterialTheme.colorScheme.primary
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    Canvas(modifier) {
        if (status == PackStatus.INSTALLED) {
            drawRect(primary)
        } else {
            drawCircle(muted, radius = size.minDimension / 2 - 2.dp.toPx(), style = Stroke(2.dp.toPx()))
        }
    }
}

/** Metropolitan France, equirectangular at 46.5° N: borders, then the areas. */
@Composable
private fun FranceMap(
    state: DomainsState,
    modifier: Modifier = Modifier,
) {
    val border = MaterialTheme.colorScheme.outlineVariant
    val dot = MaterialTheme.colorScheme.onSurfaceVariant
    val hit = MaterialTheme.colorScheme.primary
    val bg = MaterialTheme.colorScheme.surfaceContainer
    val description = pluralStringResource(R.plurals.domains_map, state.all.size, state.all.size)
    val highlighted =
        remember(state.results, state.all) {
            if (state.results.size ==
                state.all.size
            ) {
                emptySet()
            } else {
                state.results.map { it.id }.toSet()
            }
        }
    Box(modifier.semantics { contentDescription = description }.testTag(DomainTags.MAP)) {
        Canvas(Modifier.fillMaxSize()) {
            drawRect(bg)
            val proj = Projection(size)
            for (ring in state.outline) {
                val path = Path()
                var i = 0
                while (i + 1 < ring.size) {
                    val p = proj(ring[i], ring[i + 1])
                    if (i == 0) path.moveTo(p.x, p.y) else path.lineTo(p.x, p.y)
                    i += 2
                }
                path.close()
                drawPath(path, border, style = Stroke(1.dp.toPx()))
            }
            for (d in state.all) {
                val p = proj(d.lon, d.lat)
                when {
                    d.status == PackStatus.INSTALLED -> drawInstalled(p, hit)
                    d.id in highlighted -> drawCircle(hit, radius = 4.dp.toPx(), center = p)
                    else -> drawCircle(dot.copy(alpha = 0.6f), radius = 2.dp.toPx(), center = p)
                }
            }
        }
    }
}

private fun DrawScope.drawInstalled(
    p: Offset,
    color: Color,
) {
    val s = 9.dp.toPx()
    val halo = 2.dp.toPx()
    drawRect(Color.White, topLeft = Offset(p.x - s / 2 - halo, p.y - s / 2 - halo), size = Size(s + 2 * halo, s + 2 * halo))
    drawRect(color, topLeft = Offset(p.x - s / 2, p.y - s / 2), size = Size(s, s))
}

/** Longitude and latitude to canvas pixels over metropolitan France. */
private class Projection(
    size: Size,
) {
    private val kx = cos(Math.toRadians(MID_LAT))
    private val scale = min(size.width / ((EAST - WEST) * kx), size.height / (NORTH - SOUTH))
    private val dx = (size.width - (EAST - WEST) * kx * scale) / 2
    private val dy = (size.height - (NORTH - SOUTH) * scale) / 2

    operator fun invoke(
        lon: Double,
        lat: Double,
    ) = Offset((dx + (lon - WEST) * kx * scale).toFloat(), (dy + (NORTH - lat) * scale).toFloat())

    private companion object {
        const val WEST = -5.2
        const val EAST = 9.6
        const val SOUTH = 41.3
        const val NORTH = 51.15
        const val MID_LAT = 46.5
    }
}
