package fr.skimap.app.community

import android.content.Context
import fr.skimap.community.AreaContext
import fr.skimap.community.Challenge
import fr.skimap.community.ChallengeKind
import fr.skimap.community.CommunityCore
import fr.skimap.community.Correction
import fr.skimap.community.ImportSummary
import fr.skimap.community.OsmNote
import fr.skimap.community.Outing
import fr.skimap.core.DomainRef
import fr.skimap.core.challengesCompute
import fr.skimap.core.listPacks
import fr.skimap.core.openPacks
import fr.skimap.core.osmNoteDraft
import fr.skimap.core.outingsList
import fr.skimap.core.stravaImport
import java.io.File
import fr.skimap.core.ChallengeKind as CoreKind

/**
 * The module :community over the Rust core (crates/ffi/src/community.rs): outings,
 * challenges, Strava export, OSM notes, and the area of a pack with the validated « fond »
 * style instantiated on it. The calibration of the groups (C11) learns the imported
 * outings through [learning].
 */
class RustCommunityCore(
    private val packsDir: () -> File,
    private val filesDir: () -> File,
    private val styleJson: () -> String,
    private val learning: DayStoreLearning,
) : CommunityCore {
    constructor(context: Context, packsDir: () -> File) : this(
        packsDir,
        { context.filesDir },
        { context.assets.open("styles/fond-clair.json").use { it.readBytes().decodeToString() } },
        DayStoreLearning(context),
    )

    override fun importStrava(
        zip: File,
        outRoot: File,
    ): ImportSummary {
        val r = stravaImport(zip.absolutePath, outRoot.absolutePath, learning.skating())
        val learned = learning.learn(r.newDirs.map { File(it, "observation.json") }.filter { it.isFile })
        return ImportSummary(
            imported = r.imported.toInt(),
            already = r.already.toInt(),
            withoutTrack = r.withoutTrack.toInt(),
            otherSports = r.otherSports.toInt(),
            observations = r.observations.toInt(),
            learned = learned,
            errors = r.errors,
        )
    }

    override fun outings(roots: List<File>): List<Outing> =
        outingsList(roots.map { it.absolutePath }).map {
            Outing(it.id, it.source, it.name, it.startMs, it.km, it.movingMin, it.gainM)
        }

    override fun challenges(
        roots: List<File>,
        routesJson: String,
        domains: List<AreaContext>,
        nowMs: Long,
    ): List<Challenge> =
        challengesCompute(
            roots.map { it.absolutePath },
            routesJson,
            domains.mapNotNull { d -> d.pistesPath?.let { DomainRef(d.siteId, d.name, it) } },
            nowMs,
        ).map {
            Challenge(
                id = it.id,
                kind =
                    when (it.kind) {
                        CoreKind.ROUTE -> ChallengeKind.ROUTE
                        CoreKind.DOMAIN -> ChallengeKind.DOMAIN
                        CoreKind.DISTANCE -> ChallengeKind.DISTANCE
                    },
                title = it.title,
                doneKm = it.doneKm,
                totalKm = it.totalKm,
                share = it.share,
                outings = it.outings.toInt(),
                doneOn = it.doneOn,
                attribution = it.attribution,
            )
        }

    override fun osmNote(
        correction: Correction,
        pisteName: String?,
        osmWayId: Long?,
        comment: String,
        lon: Double,
        lat: Double,
        french: Boolean,
    ): OsmNote {
        val n = osmNoteDraft(correction.id, pisteName, osmWayId, comment, lon, lat, french)
        return OsmNote(n.lat, n.lon, n.text, n.formBody)
    }

    override fun area(siteId: String?): AreaContext? {
        val packs = listPacks(packsDir().absolutePath).filter { it.error == null && it.bounds != null }
        val e = packs.firstOrNull { it.siteId == siteId } ?: packs.firstOrNull() ?: return null
        val b = checkNotNull(e.bounds)
        val style =
            runCatching {
                val view = openPacks(listOf(e.dir), styleJson(), false)
                try {
                    view.style.json
                } finally {
                    view.set.destroy()
                }
            }.getOrNull()
        val pistes = File(e.dir, "layers/pistes.geojson").takeIf { it.isFile }
        return AreaContext(e.siteId, e.siteName, doubleArrayOf(b.west, b.south, b.east, b.north), style, pistes?.absolutePath)
    }

    override fun outingRoots(): List<File> = listOf(File(filesDir(), "plans"), File(filesDir(), "outings"))
}
