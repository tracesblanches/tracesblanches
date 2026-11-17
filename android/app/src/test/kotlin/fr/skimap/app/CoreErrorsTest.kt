package fr.skimap.app

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import fr.skimap.app.ui.CoreErrors
import fr.skimap.core.TrackingException
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The typed errors of the core said with the resources, in English and in French (VO6). */
@RunWith(AndroidJUnit4::class)
class CoreErrorsTest {
    private fun text(e: Throwable) = CoreErrors.text(e, ApplicationProvider.getApplicationContext<android.content.Context>().resources)

    private val errors =
        listOf(
            TrackingException.Invalid("No passable cell within 200 m"),
            TrackingException.Unsupported("x"),
            TrackingException.NoMorning(),
            TrackingException.Closed(),
            TrackingException.NotAnOuting("/plans/x"),
            TrackingException.BadFixes(25uL),
            TrackingException.BadCarPosition(),
            TrackingException.NoTrailGraph(),
            IllegalStateException("other"),
        )

    @Test
    @Config(qualifiers = "en")
    fun inEnglish() {
        assertEquals(
            listOf(
                "No passable cell within 200 m",
                "No day plan for this area (x)",
                "No loop computed yet",
                "Tracking has stopped",
                "This folder is not an outing",
                "Unreadable positions (size 25)",
                "Invalid car position",
                "This pack has no trail network",
                "other",
            ),
            errors.map(::text),
        )
    }

    @Test
    @Config(qualifiers = "fr")
    fun inFrench() {
        assertEquals(
            listOf(
                "No passable cell within 200 m",
                "Pas de plan de journée pour ce domaine (x)",
                "Aucune boucle calculée",
                "Suivi arrêté",
                "Ce dossier n'est pas une sortie",
                "Positions illisibles (taille 25)",
                "Position de la voiture invalide",
                "Ce pack n'a pas de réseau de pistes",
                "other",
            ),
            errors.map(::text),
        )
    }
}
