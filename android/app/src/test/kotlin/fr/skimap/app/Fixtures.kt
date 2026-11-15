package fr.skimap.app

import fr.skimap.app.data.OpenedPack
import fr.skimap.app.data.PackRepository
import fr.skimap.app.data.SettingsStore
import fr.skimap.core.BasemapInfo
import fr.skimap.core.Bounds
import fr.skimap.core.LayerToggle
import fr.skimap.core.LegendEntry
import fr.skimap.core.MapStyle
import fr.skimap.core.MapSymbol
import fr.skimap.core.OverlayInfo
import fr.skimap.core.PackEntry
import fr.skimap.core.PackException
import fr.skimap.core.PackInfo
import fr.skimap.core.StatusSummary
import fr.skimap.core.SymbolShape
import java.io.File

/** Values close to the real Cervières pack, without the Rust library. */
object Fixtures {
    const val CERVIERES = "/packs/cervieres"
    const val CHAPELLE = "/packs/chapelle-des-bois"

    fun entry(
        dir: String,
        name: String,
        error: String? = null,
    ) = PackEntry(dir, dir.substringAfterLast('/'), name, "20261115T014538Z", error, Bounds(6.66, 44.77, 6.86, 44.92))

    fun info(
        dir: String = CERVIERES,
        name: String = "Cervières",
        status: StatusSummary? = StatusSummary(0u, 8u, 0u, "2026-11-15T01:45:36Z"),
        validUntil: String? = "2026-11-17",
    ) = PackInfo(
        dir = dir,
        siteId = dir.substringAfterLast('/'),
        siteName = name,
        department = "05",
        packVersion = "20261115T014538Z",
        generatedAt = "2026-11-15T01:45:38Z",
        validUntil = validUntil,
        validityNote = "Pack à reconstruire avant le 2026-11-17.",
        bounds = Bounds(6.66, 44.77, 6.86, 44.92),
        centerLon = 6.7746,
        centerLat = 44.8586,
        basemaps = listOf(BasemapInfo("plan-ign", "Plan IGN v2", 0u, 16u), BasemapInfo("osm", "OpenStreetMap (Protomaps)", 0u, 15u)),
        overlays = listOf(OverlayInfo("pistes", "Pistes de ski de fond", 196u, false)),
        status = status,
        personalSources = true,
        attributions = listOf("© IGN – Plan IGN v2", "© les contributeurs d'OpenStreetMap"),
    )

    fun style(basemap: String = "plan-ign") =
        MapStyle(
            json = """{"version":8,"name":"skimap test $basemap","sources":{},"layers":[]}""",
            basemap = basemap,
            font = "Source Sans Pro Bold",
            overlays = emptyList(),
            attributions =
                listOf(
                    "© IGN – Plan IGN v2, mise à jour du 2026-10-08, Licence Ouverte Etalab 2.0",
                    "Nordic France",
                    "© les contributeurs d'OpenStreetMap (ODbL), extrait quotidien OpenSnowMap.org",
                    "© les contributeurs de refuges.info (CC BY-SA 2.0)",
                ),
            toggles =
                listOf(
                    LayerToggle("refuges", "Refuges, cabanes et abris", listOf("skimap-refuges-info-icon"), true),
                    LayerToggle("water", "Points d'eau", listOf("skimap-refuges-info-water"), true),
                    LayerToggle("no-network", "Zones sans réseau mobile (112 compris)", listOf("skimap-no-network-fill"), false),
                    LayerToggle("place-names", "Noms de lieux et symboles du fond", listOf("label"), true),
                ),
            attributionShort =
                "© IGN – Plan IGN v2 · Nordic France · © les contributeurs d'OpenStreetMap · " +
                    "© les contributeurs de refuges.info",
        )

    val symbols =
        listOf(
            MapSymbol("skimap-difficulty-novice", SymbolShape.CIRCLE, "#00703c", "#ffffff", 18u),
            MapSymbol("skimap-difficulty-easy", SymbolShape.SQUARE, "#0047ab", "#ffffff", 18u),
            MapSymbol("skimap-difficulty-intermediate", SymbolShape.DIAMOND, "#c8102e", "#ffffff", 18u),
            MapSymbol("skimap-difficulty-advanced", SymbolShape.DOUBLE_DIAMOND, "#111111", "#ffffff", 18u),
            MapSymbol("skimap-difficulty-unknown", SymbolShape.RING, "#5c5c5c", "#ffffff", 16u),
            MapSymbol("skimap-oneway", SymbolShape.ARROW, "#111111", "#ffffff", 20u),
            MapSymbol("skimap-status-closed", SymbolShape.CROSS, "#a00000", "#ffffff", 22u),
            MapSymbol("skimap-status-open", SymbolShape.CHECK, "#006b3c", "#ffffff", 22u),
            MapSymbol("skimap-refuge", SymbolShape.HOUSE, "#5b2c06", "#ffffff", 28u),
            MapSymbol("skimap-water", SymbolShape.DROP, "#0b4f8a", "#ffffff", 22u),
        )

    val legend =
        listOf(
            LegendEntry("Pistes", "Verte : très facile", "skimap-difficulty-novice", "#00703c", false),
            LegendEntry("Pistes", "Bleue : facile", "skimap-difficulty-easy", "#0047ab", false),
            LegendEntry("Pistes", "Rouge : difficile", "skimap-difficulty-intermediate", "#c8102e", false),
            LegendEntry("Pistes", "Noire : très difficile", "skimap-difficulty-advanced", "#111111", false),
            LegendEntry("Pistes", "Difficulté inconnue", "skimap-difficulty-unknown", "#5c5c5c", false),
            LegendEntry("Pistes", "Pointillés : raquettes et piétons", null, "#5c5c5c", true),
            LegendEntry("Pistes", "Flèches : sens unique", "skimap-oneway", null, false),
            LegendEntry("État des pistes", "Croix : itinéraire fermé", "skimap-status-closed", "#a00000", true),
            LegendEntry("État des pistes", "Coche : itinéraire ouvert", "skimap-status-open", "#006b3c", false),
            LegendEntry("Refuges", "Refuge, cabane ou abri", "skimap-refuge", null, false),
        )

    fun opened(
        dir: String = CERVIERES,
        name: String = "Cervières",
        basemap: String = "plan-ign",
    ) = OpenedPack(info(dir, name), style(basemap), legend, symbols, "skimap-ffi 0.1.0")
}

/** In-memory [PackRepository]: two packs, Chapelle-des-Bois opens, a broken one does not. */
class FakeRepository(
    var entries: List<PackEntry> =
        listOf(
            Fixtures.entry(Fixtures.CERVIERES, "Cervières"),
            Fixtures.entry(Fixtures.CHAPELLE, "Chapelle-des-Bois"),
        ),
    private val broken: Set<String> = emptySet(),
) : PackRepository {
    override val packsDir = File("/packs")
    val opened = mutableListOf<String>()
    val styles = mutableListOf<String>()
    private var current: String? = null

    override suspend fun list(): List<PackEntry> = entries

    override suspend fun open(
        dirs: List<String>,
        basemap: String?,
    ): OpenedPack {
        // As the core: packs that do not open are skipped when another one opens.
        val ok = dirs.filterNot { it in broken }
        if (ok.isEmpty()) throw PackException.Invalid("manifest.json: EOF")
        val dir = ok.first()
        opened += dirs.joinToString("+")
        current = dir
        if (ok.size > 1) return Fixtures.opened("", "Tous les domaines installés", basemap ?: "plan-ign")
        val name = entries.first { it.dir == dir }.siteName
        return Fixtures.opened(dir, name, basemap ?: "plan-ign")
    }

    override suspend fun style(basemap: String): MapStyle {
        if (basemap == "missing") throw PackException.UnknownBasemap(basemap)
        styles += basemap
        return Fixtures.style(basemap)
    }
}

class MemorySettings(
    var value: String? = null,
) : SettingsStore {
    val hidden = mutableMapOf<String, Set<String>>()

    override fun get() = value

    override fun set(dir: String) {
        value = dir
    }

    override fun hiddenLayers(style: String) = hidden[style]

    override fun setHiddenLayers(
        style: String,
        hidden: Set<String>,
    ) {
        this.hidden[style] = hidden
    }
}
