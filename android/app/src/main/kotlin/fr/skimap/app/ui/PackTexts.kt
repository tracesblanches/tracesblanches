package fr.skimap.app.ui

import fr.skimap.core.PackInfo
import fr.skimap.core.StatusSummary
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.util.Locale

/** Validity of a pack on a given day. */
enum class Validity { VALID, LAST_DAY, EXPIRED, UNKNOWN }

/** French texts of the header, computed without Android so that they are unit tested. */
object PackTexts {
    private val day = DateTimeFormatter.ofPattern("d MMMM yyyy", Locale.FRENCH)
    private val dayShort = DateTimeFormatter.ofPattern("dd/MM", Locale.FRENCH)
    private val time = DateTimeFormatter.ofPattern("HH:mm", Locale.FRENCH)

    fun validity(
        info: PackInfo,
        today: LocalDate,
    ): Validity {
        val until = info.validUntil?.let(::parseDate) ?: return Validity.UNKNOWN
        return when {
            today.isAfter(until) -> Validity.EXPIRED
            today.isEqual(until) -> Validity.LAST_DAY
            else -> Validity.VALID
        }
    }

    /** `Valable jusqu'au 17 novembre 2026`, `Périmé depuis le 17 novembre 2026`... */
    fun validityText(
        info: PackInfo,
        today: LocalDate,
    ): String {
        val until = info.validUntil?.let(::parseDate)?.format(day)
        return when (validity(info, today)) {
            Validity.VALID -> "Valable jusqu'au $until"
            Validity.LAST_DAY -> "Dernier jour de validité : $until"
            Validity.EXPIRED -> "Périmé depuis le $until : mettez le pack à jour"
            Validity.UNKNOWN -> "Validité inconnue"
        }
    }

    /** `2 ouvertes, 40 fermées` (map chip). */
    fun statusShort(status: StatusSummary?): String {
        if (status == null) return "état des pistes absent"
        val parts = mutableListOf<String>()
        parts += plural(status.open.toInt(), "ouverte", "ouvertes")
        parts += plural(status.closed.toInt(), "fermée", "fermées")
        if (status.unknown > 0u) parts += "${status.unknown} sans état"
        return parts.joinToString(", ")
    }

    /** `Pistes : 2 ouvertes, 40 fermées · relevé le 15/11 à 02:46`. */
    fun statusText(
        status: StatusSummary?,
        zone: ZoneId,
    ): String {
        if (status == null) return "État des pistes absent de ce pack"
        val polled =
            status.polledAt?.let(::parseInstant)?.atZone(zone)?.let {
                " · relevé le ${it.format(dayShort)} à ${it.format(time)}"
            } ?: ""
        return "Pistes : ${statusShort(status)}$polled"
    }

    private fun plural(
        n: Int,
        one: String,
        many: String,
    ) = "$n ${if (n > 1) many else one}"

    private fun parseDate(s: String): LocalDate? =
        try {
            LocalDate.parse(s.take(DATE_LENGTH))
        } catch (_: DateTimeParseException) {
            null
        }

    private fun parseInstant(s: String): Instant? =
        try {
            Instant.parse(s)
        } catch (_: DateTimeParseException) {
            null
        }

    private const val DATE_LENGTH = 10
}
