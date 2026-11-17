package fr.skimap.app

import fr.skimap.core.Lang
import java.util.Locale

/**
 * The language of the texts of the core (VO6): French on a French phone, English for every
 * other language. Every core call that returns texts takes it.
 */
fun appLang(locale: Locale = Locale.getDefault()): Lang = if (locale.language == "fr") Lang.FR else Lang.EN

/**
 * The locale the Android voice reads the texts of [appLang] with: French, the phone's own
 * English, else British English (a phone in another language gets the English texts).
 */
fun voiceLocale(locale: Locale = Locale.getDefault()): Locale =
    when {
        appLang(locale) == Lang.FR -> Locale.FRANCE
        locale.language == "en" -> locale
        else -> Locale.UK
    }

private const val MINUTES_PER_HOUR = 60

/** A duration in minutes as the core writes it: « 1 h 30 », « 2 h », « 45 min »; « 1 h 30 min » in English. */
fun durationText(
    min: Int,
    lang: Lang = appLang(),
): String {
    val h = min / MINUTES_PER_HOUR
    val m = min % MINUTES_PER_HOUR
    return when {
        h == 0 -> "$m min"
        m == 0 -> "$h h"
        lang == Lang.FR -> "$h h ${"%02d".format(Locale.ROOT, m)}"
        else -> "$h h ${"%02d".format(Locale.ROOT, m)} min"
    }
}

/** A clock time `HH:MM` as the language writes it: « 15 h », « 15 h 30 »; « 15:00 » in English. */
fun clockText(
    hhmm: String,
    lang: Lang = appLang(),
): String {
    if (lang != Lang.FR) return hhmm
    val (h, m) = hhmm.split(":").let { it[0].toInt() to it.getOrNull(1).orEmpty() }
    return if (m.isEmpty() || m == "00") "$h h" else "$h h $m"
}
