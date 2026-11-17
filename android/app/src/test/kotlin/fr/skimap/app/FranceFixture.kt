package fr.skimap.app

import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest

/**
 * A France basemap directory as the pipeline writes it (`skimap_pack::france`, X20):
 * `france.json` naming `france.pmtiles` and the Regular glyphs, with their SHA-256.
 * The archive holds [tiles] (the core does not read tiles, MapLibre does).
 */
object FranceFixture {
    fun sha256(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

    fun write(
        dir: File,
        tiles: String = "tiles",
        build: String = "20261117.pmtiles",
    ): File {
        val files =
            mapOf(
                "france.pmtiles" to tiles.toByteArray(),
                "fonts/Source Sans Pro Regular/0-255.pbf" to "glyphs".toByteArray(),
            )
        val entries = JSONArray()
        for ((path, bytes) in files.toSortedMap()) {
            File(dir, path).apply { parentFile.mkdirs() }.writeBytes(bytes)
            entries.put(JSONObject().put("path", path).put("bytes", bytes.size).put("sha256", sha256(bytes)))
        }
        val description =
            JSONObject()
                .put("format", "skimap-france-basemap")
                .put("format_version", 1)
                .put("build", build)
                .put("schema", "4.15.2")
                .put("data_date", "2026-11-17")
                .put("bbox", JSONArray(listOf(-5.5, 41.2, 9.8, 51.3)))
                .put("minzoom", 0)
                .put("maxzoom", 6)
                .put("files", entries)
                .put(
                    "sources",
                    JSONArray().put(
                        JSONObject()
                            .put("name", "Protomaps")
                            .put("url", "https://build.protomaps.com/$build")
                            .put("licence", "ODbL 1.0")
                            .put("attribution", "© les contributeurs d'OpenStreetMap, Protomaps"),
                    ),
                )
        File(dir, "france.json").writeText(description.toString())
        return dir
    }
}
