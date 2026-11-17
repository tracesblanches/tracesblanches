package fr.skimap.app

import fr.skimap.app.data.CameraSpot
import fr.skimap.app.data.DownloadState
import fr.skimap.app.data.Downloads
import fr.skimap.app.data.OpenedPack
import fr.skimap.app.data.PackRepository
import fr.skimap.app.data.SettingsStore
import fr.skimap.app.data.StyleChoice
import fr.skimap.app.data.StyleSource
import fr.skimap.core.BasemapInfo
import fr.skimap.core.Bounds
import fr.skimap.core.LayerGroup
import fr.skimap.core.LegendEntry
import fr.skimap.core.LegendKind
import fr.skimap.core.MapStyle
import fr.skimap.core.PackEntry
import fr.skimap.core.PackException
import fr.skimap.core.PackInfo
import fr.skimap.core.ServerSite
import fr.skimap.core.StatusSummary
import kotlinx.coroutines.flow.MutableStateFlow
import org.json.JSONObject
import java.io.File

/** Values close to the real Cervières pack, without the Rust library. */
object Fixtures {
    const val CERVIERES = "/packs/cervieres"
    const val CHAPELLE = "/packs/chapelle-des-bois"

    fun entry(
        dir: String,
        name: String,
        error: String? = null,
        version: String = "20261115T014538Z",
    ) = PackEntry(dir, dir.substringAfterLast('/'), name, version, error, Bounds(6.66, 44.77, 6.86, 44.92))

    fun info(
        dir: String = CERVIERES,
        name: String = "Cervières",
        status: StatusSummary? = StatusSummary(0u, 8u, 0u, "2026-11-15T01:45:36Z"),
        validUntil: String? = "2026-11-17",
    ) = PackInfo(
        dir = dir,
        siteId = dir.substringAfterLast('/').ifEmpty { "libre" },
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
        status = status,
        personalSources = true,
        attributions = listOf("© IGN – Plan IGN v2", "© les contributeurs d'OpenStreetMap"),
    )

    /** Groups of the fond and SRN styles (`metadata.skimap:groups` of `styles/`). */
    fun groups(family: String): List<LayerGroup> {
        fun g(
            id: String,
            toggleable: Boolean,
            visible: Boolean = true,
        ) = LayerGroup(id, id, toggleable, visible, listOf("$id-layer"))
        val safety = listOf(g("fond", false), g("pistes", false), g("dangers", false), g("position", false))
        return when (family) {
            "srn" -> {
                safety +
                    listOf(
                        g("reglementation", false),
                        g("pentes", true),
                        g("refuges", true),
                        g("points-eau", true),
                        g("acces", true),
                        g("noms", true),
                        g("reseau", true),
                        g("neige", true, visible = false),
                    )
            }

            "guidage" -> {
                listOf(g("fond", false), g("pistes", false), g("itineraire", false), g("position", false), g("noms", true))
            }

            else -> {
                safety + listOf(g("refuges", true), g("acces", true), g("noms", true), g("neige", true, visible = false))
            }
        }
    }

    val legend =
        listOf(
            LegendEntry("pistes", "novice", "#008c45", LegendKind.LINE),
            LegendEntry("pistes", "easy", "#0062c4", LegendKind.LINE),
            LegendEntry("pistes", "intermediate", "#d81e1e", LegendKind.LINE),
            LegendEntry("pistes", "advanced", "#111111", LegendKind.LINE),
            LegendEntry("pistes", "unknown", "#6b6b6b", LegendKind.LINE),
            LegendEntry("pistes", "closed", "#0062c4", LegendKind.DASHED),
            LegendEntry("pistes", "backcountry", "#0062c4", LegendKind.DASHED),
            LegendEntry("pistes", "hike", "#6a5f55", LegendKind.DOTTED),
        )

    val srnLegend =
        legend +
            listOf(
                LegendEntry("pentes", "slope-1", "#f2e500", LegendKind.AREA),
                LegendEntry("pentes", "slope-2", "#f39419", LegendKind.AREA),
                LegendEntry("pentes", "slope-3", "#f00000", LegendKind.AREA),
                LegendEntry("pentes", "slope-4", "#c889bb", LegendKind.AREA),
                LegendEntry("dangers", "exposure-2", "#f59628", LegendKind.AREA),
                LegendEntry("dangers", "exposure-3", "#c81e1e", LegendKind.AREA),
            )

    fun style(
        family: String = "fond",
        theme: String = "clair",
        site: String = "cervieres",
    ) = MapStyle(
        json = """{"version":8,"name":"skimap $site $family-$theme","sources":{},"layers":[]}""",
        name = "skimap $site $family-$theme",
        styleId = family,
        theme = theme,
        groups = groups(family),
        overlays = emptyList(),
        attributions =
            listOf(
                "© IGN – Plan IGN v2, mise à jour du 2026-10-08, Licence Ouverte Etalab 2.0",
                "Nordic France",
                "© les contributeurs d'OpenStreetMap (ODbL), extrait quotidien OpenSnowMap.org",
                "© les contributeurs de refuges.info (CC BY-SA 2.0)",
            ),
        legend =
            when (family) {
                "srn" -> srnLegend
                "guidage" -> emptyList()
                else -> legend
            },
        missing = emptyList(),
    )

    fun opened(
        dir: String = CERVIERES,
        name: String = "Cervières",
        family: String = "fond",
        theme: String = "clair",
    ) = OpenedPack(info(dir, name), style(family, theme, dir.substringAfterLast('/').ifEmpty { "libre" }), "skimap-ffi 0.1.0")

    fun site(
        id: String,
        version: String = "20261116T060000Z",
        bytes: Long = 43_500_000,
    ) = ServerSite(
        siteId = id,
        name = id,
        bbox = listOf(6.66, 44.77, 6.86, 44.92),
        indexUrl = "/v1/sites/$id/index.json",
        latestVersion = version,
        publishedAt = "2026-11-16T06:00:00Z",
        firstDownloadBytes = bytes.toULong(),
        packBytes = bytes.toULong(),
        voice = emptyList(),
    )
}

/** Style source of the tests: a minimal style JSON naming the choice. */
class FakeStyles : StyleSource {
    val read = mutableListOf<String>()

    override suspend fun read(choice: StyleChoice): String {
        read += choice.asset
        return """{"metadata":{"skimap:style":"${choice.family}","skimap:theme":"${if (choice.dark) "sombre" else "clair"}"}}"""
    }
}

/** In-memory [PackRepository]: two packs, Chapelle-des-Bois opens, a broken one does not. */
class FakeRepository(
    var entries: List<PackEntry> =
        listOf(
            Fixtures.entry(Fixtures.CERVIERES, "Cervières"),
            Fixtures.entry(Fixtures.CHAPELLE, "Chapelle-des-Bois"),
        ),
    private val broken: Set<String> = emptySet(),
    /** Whether the phone has the France basemap (X20). */
    var france: Boolean = false,
) : PackRepository {
    override val packsDir = File("/packs")
    val opened = mutableListOf<String>()
    val styles = mutableListOf<String>()
    private var site = "cervieres"

    private fun meta(styleJson: String): Pair<String, String> {
        val m = JSONObject(styleJson).getJSONObject("metadata")
        return m.getString("skimap:style") to m.getString("skimap:theme")
    }

    override suspend fun list(): List<PackEntry> = entries

    override suspend fun open(
        dirs: List<String>,
        styleJson: String,
        free: Boolean,
    ): OpenedPack {
        // As the core: packs that do not open are skipped when another one opens.
        val ok = dirs.filterNot { it in broken }
        // As the core: the free mode opens without any pack over the France basemap.
        val overFrance = free && france && dirs.isEmpty()
        if (ok.isEmpty() && !overFrance) throw PackException.Invalid("manifest.json: EOF")
        val dir = ok.firstOrNull().orEmpty()
        opened += dirs.joinToString("+")
        val (family, theme) = meta(styleJson)
        styles += "$family-$theme"
        if (free) {
            site = "libre"
            return Fixtures.opened("", "Tous les domaines installés", family, theme)
        }
        site = dir.substringAfterLast('/')
        val name = entries.first { it.dir == dir }.siteName
        return Fixtures.opened(dir, name, family, theme)
    }

    override suspend fun hasFrance(): Boolean = france

    override suspend fun style(styleJson: String): MapStyle {
        val (family, theme) = meta(styleJson)
        if (family == "broken") throw PackException.Invalid("style JSON: EOF")
        styles += "$family-$theme"
        return Fixtures.style(family, theme, site)
    }
}

class MemorySettings(
    var value: String? = null,
    var practiceValue: String? = null,
) : SettingsStore {
    val hidden = mutableMapOf<String, Set<String>>()
    var auto = true
    var cameraSpot: CameraSpot? = null

    override fun get() = value

    override fun set(dir: String) {
        value = dir
    }

    override fun practice() = practiceValue

    override fun setPractice(practice: String) {
        practiceValue = practice
    }

    override fun hiddenLayers(style: String) = hidden[style]

    override fun setHiddenLayers(
        style: String,
        hidden: Set<String>,
    ) {
        this.hidden[style] = hidden
    }

    override fun autoUpdate() = auto

    override fun setAutoUpdate(on: Boolean) {
        auto = on
    }

    override fun camera() = cameraSpot

    override fun setCamera(spot: CameraSpot) {
        cameraSpot = spot
    }
}

/** [Downloads] without WorkManager: records the calls, states set by the test. */
class FakeDownloads(
    var sites: List<ServerSite> = listOf(Fixtures.site("cervieres"), Fixtures.site("les-rousses")),
    var online: Boolean = true,
) : Downloads {
    override val states = MutableStateFlow<Map<String, DownloadState>>(emptyMap())
    val calls = mutableListOf<String>()

    override suspend fun cachedSites() = sites

    override suspend fun refreshSites() = if (online) sites else null

    override fun download(
        site: ServerSite,
        mobileData: Boolean,
    ) {
        calls += "download ${site.siteId} ${if (mobileData) "mobile" else "wifi"}"
    }

    override fun cancel(site: String) {
        calls += "cancel $site"
    }

    override suspend fun remove(site: String) {
        calls += "remove $site"
    }
}
