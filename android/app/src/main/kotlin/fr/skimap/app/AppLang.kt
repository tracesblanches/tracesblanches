package fr.skimap.app

import fr.skimap.core.Lang
import java.util.Locale

/**
 * The language of the texts of the core (VO6): French on a French phone, English for every
 * other language. Every core call that returns texts takes it.
 */
fun appLang(locale: Locale = Locale.getDefault()): Lang = if (locale.language == "fr") Lang.FR else Lang.EN
