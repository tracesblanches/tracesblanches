package fr.skimap.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

/**
 * Every visible string of the app and its modules exists in English (`res/values`, the
 * default) and in French (`res/values-fr`), with the same placeholders (VO6). Fails on a
 * string missing from either language.
 */
class StringsTest {
    private val modules = listOf("app", "tracking", "sport", "community", "group", "sync")

    /** Translatable strings, plurals and arrays of a resource file: name to placeholders. */
    private fun strings(file: File): Map<String, List<String>> {
        if (!file.isFile) return emptyMap()
        val doc = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
        val out = sortedMapOf<String, List<String>>()
        for (tag in listOf("string", "plurals", "string-array")) {
            val nodes = doc.getElementsByTagName(tag)
            for (i in 0 until nodes.length) {
                val e = nodes.item(i) as Element
                if (e.getAttribute("translatable") == "false") continue
                assertTrue("${file.path}: ${e.getAttribute("name")} is empty", e.textContent.isNotBlank())
                out[e.getAttribute("name")] =
                    PLACEHOLDER
                        .findAll(e.textContent)
                        .map { it.value }
                        .toSortedSet()
                        .toList()
            }
        }
        return out
    }

    private fun res(module: String) = File(System.getProperty("user.dir")).resolve("../$module/src/main/res")

    @Test
    fun everyStringInBothLanguages() {
        var total = 0
        for (m in modules) {
            val dir = res(m)
            val en = strings(File(dir, "values/strings.xml"))
            val fr = strings(File(dir, "values-fr/strings.xml"))
            assertEquals("$m: strings only in English", emptySet<String>(), en.keys - fr.keys)
            assertEquals("$m: strings only in French", emptySet<String>(), fr.keys - en.keys)
            for ((k, p) in en) assertEquals("$m: placeholders of $k", p, fr[k])
            total += en.size
        }
        assertTrue("strings found: $total", total > 200)
    }

    @Test
    fun noOtherLanguageFolder() {
        // Two languages only: a third folder would be a language half translated.
        for (m in modules) {
            val folders =
                res(m)
                    .listFiles()
                    ?.map { it.name }
                    ?.filter { it.startsWith("values") && File(res(m), "$it/strings.xml").isFile }
                    .orEmpty()
                    .toSet()
            assertTrue("$m: $folders", folders.isEmpty() || folders == setOf("values", "values-fr"))
        }
    }

    private companion object {
        val PLACEHOLDER = Regex("%(\\d+\\$)?[sdf]")
    }
}
