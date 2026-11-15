package fr.skimap.app

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import fr.skimap.app.ui.PackTexts
import fr.skimap.app.ui.Validity
import fr.skimap.app.ui.theme.SkiDarkColors
import fr.skimap.app.ui.theme.SkiLightColors
import fr.skimap.core.StatusSummary
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Contrast of the colour schemes (WCAG) and French texts of the header. */
class ThemeAndTextsTest {
    private fun contrast(
        a: Color,
        b: Color,
    ): Double {
        val (hi, lo) = listOf(a.luminance(), b.luminance()).sortedDescending()
        return (hi + 0.05) / (lo + 0.05)
    }

    /** Text / background pairs the app uses; each reaches 7:1 (WCAG AAA). */
    private fun pairs(c: ColorScheme) =
        mapOf(
            "onSurface/surface" to (c.onSurface to c.surface),
            "onSurfaceVariant/surfaceVariant" to (c.onSurfaceVariant to c.surfaceVariant),
            "onSurface/surfaceContainerLow (sheet)" to (c.onSurface to c.surfaceContainerLow),
            "onPrimary/primary (primary action)" to (c.onPrimary to c.primary),
            "onPrimaryContainer/primaryContainer" to (c.onPrimaryContainer to c.primaryContainer),
            "onSecondaryContainer/secondaryContainer" to (c.onSecondaryContainer to c.secondaryContainer),
            "onErrorContainer/errorContainer (expired pack)" to (c.onErrorContainer to c.errorContainer),
            "error/surface (error line)" to (c.error to c.surface),
            "outline/surface (button borders)" to (c.outline to c.surface),
        )

    @Test
    fun schemesReachAaa() {
        for ((name, scheme) in listOf("light" to SkiLightColors, "dark" to SkiDarkColors)) {
            for ((pair, colors) in pairs(scheme)) {
                val r = contrast(colors.first, colors.second)
                assertTrue("$name $pair: %.2f".format(r), r >= 7.0)
            }
        }
        assertEquals(21.0, contrast(Color.Black, Color.White), 1e-6)
    }

    @Test
    fun validity() {
        val info = Fixtures.info(validUntil = "2026-11-17")
        assertEquals(Validity.VALID, PackTexts.validity(info, TODAY))
        assertEquals(Validity.LAST_DAY, PackTexts.validity(info, TODAY.plusDays(2)))
        assertEquals(Validity.EXPIRED, PackTexts.validity(info, TODAY.plusDays(3)))
        assertEquals(Validity.UNKNOWN, PackTexts.validity(Fixtures.info(validUntil = null), TODAY))
        assertEquals("Valable jusqu'au 17 novembre 2026", PackTexts.validityText(info, TODAY))
        assertEquals("Dernier jour de validité : 17 novembre 2026", PackTexts.validityText(info, TODAY.plusDays(2)))
    }

    @Test
    fun status() {
        // 01:46 UTC is 02:46 in Paris in November.
        val s = StatusSummary(2u, 40u, 0u, "2026-11-15T01:46:00Z")
        assertEquals("Pistes : 2 ouvertes, 40 fermées · relevé le 15/11 à 02:46", PackTexts.statusText(s, PARIS))
        assertEquals("Pistes : 1 ouverte, 0 fermée, 3 sans état", PackTexts.statusText(StatusSummary(1u, 0u, 3u, "x"), PARIS))
        assertEquals("État des pistes absent de ce pack", PackTexts.statusText(null, PARIS))
    }
}
