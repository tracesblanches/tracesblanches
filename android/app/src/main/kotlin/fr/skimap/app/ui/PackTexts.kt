package fr.skimap.app.ui

import android.content.res.Resources
import fr.skimap.app.R
import fr.skimap.app.data.PackProblem
import fr.skimap.core.PackInfo
import fr.skimap.core.StatusSummary
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.time.format.FormatStyle
import java.util.Locale

/** Validity of a pack on a given day. */
enum class Validity { VALID, LAST_DAY, EXPIRED, UNKNOWN }

/**
 * Texts of the header and the sheet in the language of the app (resources: English by
 * default, French on a French phone), dates in the format of [locale].
 */
class PackTexts(
    private val res: Resources,
    locale: Locale = res.configuration.locales[0],
) {
    private val day = DateTimeFormatter.ofLocalizedDate(FormatStyle.LONG).withLocale(locale)
    private val dayShort = DateTimeFormatter.ofPattern(if (locale.language == "fr") "dd/MM" else "MMM d", locale)
    private val time = DateTimeFormatter.ofPattern("HH:mm", locale)

    /** `Valid until 17 November 2026`, `Périmé depuis le 17 novembre 2026`... */
    fun validityText(
        info: PackInfo,
        today: LocalDate,
    ): String {
        val until =
            info.validUntil
                ?.let(::parseDate)
                ?.format(day)
                .orEmpty()
        return when (validity(info, today)) {
            Validity.VALID -> res.getString(R.string.validity_valid, until)
            Validity.LAST_DAY -> res.getString(R.string.validity_last_day, until)
            Validity.EXPIRED -> res.getString(R.string.validity_expired, until)
            Validity.UNKNOWN -> res.getString(R.string.validity_unknown)
        }
    }

    /** `2 open, 40 closed` (map chip). */
    fun statusShort(status: StatusSummary?): String {
        if (status == null) return res.getString(R.string.status_absent_short)
        val parts = mutableListOf<String>()
        parts += res.getQuantityString(R.plurals.status_open, status.open.toInt(), status.open.toInt())
        parts += res.getQuantityString(R.plurals.status_closed, status.closed.toInt(), status.closed.toInt())
        if (status.unknown > 0u) parts += res.getQuantityString(R.plurals.status_unknown, status.unknown.toInt(), status.unknown.toInt())
        return parts.joinToString(", ")
    }

    /** `Pistes: 2 open, 40 closed · checked on Nov 15 at 02:46`. */
    fun statusText(
        status: StatusSummary?,
        zone: ZoneId,
    ): String {
        if (status == null) return res.getString(R.string.status_absent)
        val short = statusShort(status)
        val at = status.polledAt?.let(::parseInstant)?.atZone(zone)
        val counts = if (at == null) short else res.getString(R.string.status_polled, short, at.format(dayShort), at.format(time))
        return res.getString(R.string.status_text, counts)
    }

    /** Site name shown: the pack's, or "all installed areas" in free mode. */
    fun siteName(
        info: PackInfo,
        free: Boolean,
    ): String = if (free) res.getString(R.string.free_mode_name) else info.siteName

    /** An error of the core. */
    fun problem(p: PackProblem): String =
        when (p.kind) {
            PackProblem.Kind.NOT_FOUND -> res.getString(R.string.error_no_pack, p.detail)
            PackProblem.Kind.INVALID -> res.getString(R.string.error_invalid_pack, p.detail)
        }

    companion object {
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
}
