package fr.skimap.app

import fr.skimap.app.day.Contact
import fr.skimap.app.day.DayInputs
import fr.skimap.app.day.DayUi
import fr.skimap.core.DayPlanView
import fr.skimap.core.DayStart
import fr.skimap.core.GroupSpec
import fr.skimap.core.GroupView
import fr.skimap.core.KeyPointView
import fr.skimap.core.LoopView
import fr.skimap.core.MorningView
import fr.skimap.tracking.GpsMode
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.Phase
import fr.skimap.tracking.Proposal
import fr.skimap.tracking.Turn
import java.time.LocalDate

/**
 * The morning and the plan of Chapelle-des-Bois on 16/11/2026 as the core gave them
 * (`crates/ffi/tests/tracking.rs`, full data), shortened.
 */
object DayFixtures {
    val LAUS = DayStart("le départ des pistes de Chapelle-des-Bois", 6.1125, 46.6032)

    fun group(
        name: String,
        label: String,
        outings: UInt = 0u,
    ) = GroupView(
        GroupSpec(name, 2u, byteArrayOf(6, 9), false, "intermediate"),
        outings,
        if (outings == 0u) "a priori, aucune sortie apprise" else "$outings sorties apprises",
        label,
        "{}",
    )

    val groups =
        listOf(
            group("Famille avec enfants débutants", "2 adultes, enfants de 10 et 14 ans"),
            group("Seul", "1 adulte"),
        )

    val loops =
        listOf(
            LoopView(
                1u,
                "Chaumoz – La Norbière – Les Mortes",
                12.8,
                260.0,
                115.0,
                "2 h 35 à 6 h 10",
                "médiane 3 h 40, prévu sur 4 h 50 (côté lent), retour prévu vers 14 h 50",
                true,
                null,
                false,
                listOf(10.1, 0.0, 2.7),
                listOf("10 km de pistes ouvertes", "neige transformée qui ramollit à partir de 11 h", "vent faible"),
                listOf("Skis à écailles — retenue incertaine sur neige transformée", "Lampe frontale — retour proche du coucher du soleil"),
                listOf(6.1125, 46.6032, 6.12, 46.61),
            ),
            LoopView(
                2u,
                "Sylvain Guillaume – Combe David",
                18.2,
                340.0,
                185.0,
                "4 h 10 à 9 h 50",
                "retour entre 14 h 10 et 19 h 50",
                true,
                "Retour prévu après la limite de 16 h 00 : boucle longue pour ce groupe",
                true,
                listOf(12.5, 0.0, 5.6),
                listOf("liaison des Rouges-Truites ouverte"),
                emptyList(),
                listOf(6.1125, 46.6032, 6.10, 46.62),
            ),
        )

    val morning =
        MorningView(
            "2026-11-16",
            "10 h 00",
            listOf(
                "Chapelle-des-Bois — lundi 16 novembre 2026",
                "Domaines :",
                "1. Site nordique de Morbier (score -3,37) : 1 itinéraire ouvert sur 3.",
                "Qualité de neige (depart-des-pistes) : Au plat (1087 m) : neige humide et lourde entre 10 h et 15 h.",
                "Coucher du soleil 17 h 02.",
            ),
            listOf(
                "Qualité de neige (depart-des-pistes) : Au plat (1087 m) : neige humide et lourde entre 10 h et 15 h.",
                "Coucher du soleil 17 h 02.",
            ),
            "Meilleur départ vers 12 h 30 (retour vers 16 h 06) : neige humide et lourde puis transformée.",
            "Matin 9 h–12 h, air +5 °C, neige ≈ 0 °C, neige mouillée : classique, retenue incertaine, skis à écailles conseillés.",
            loops,
            listOf("carte d'enneigement : absente du pack"),
            "2 adultes, enfants de 10 et 14 ans",
            "Ce plan n'est jamais une garantie : informations indicatives, lisez le BERA et les sources officielles, et décidez sur place.",
            191u,
        )

    /** The groups on a phone in English. */
    val groupsEn =
        listOf(
            group("Family with beginner children", "2 adults, children of 10 and 14").copy(learned = "prior, no outing learned"),
            group("Solo", "1 adult").copy(learned = "prior, no outing learned"),
        )

    /** The same morning on a phone in English: the core's English texts (VO6). */
    val loopsEn =
        listOf(
            loops[0].copy(
                timeText = "2 h 35 to 6 h 10",
                backText = "median 3 h 40, planned on 4 h 50 (slow side), back around 14:50",
                reasons = listOf("10.1 km on open trails", "surface: transformed snow softening (corn)", "light wind (gusts 15 km/h)"),
                equipment = listOf("Waxless skis — uncertain grip on transformed snow", "Headlamp — back close to sunset"),
            ),
            loops[1].copy(
                timeText = "4 h 10 to 9 h 50",
                backText = "back between 14:10 and 19:50",
                warning = "Expected back after the limit of 16:00: a long loop for this group",
                reasons = listOf("12.5 km on open trails"),
            ),
        )

    val morningEn =
        morning.copy(
            startTime = "10:00",
            briefing =
                listOf(
                    "Chapelle-des-Bois — Monday 16 November 2026",
                    "Areas:",
                    "1. Site nordique de Morbier (score -3.37): 1 route open out of 3 (5 km out of 20).",
                    "Snow quality (depart-des-pistes): On the flat (1087 m): wet heavy snow between 10:00 and 15:00.",
                    "Sunset 17:02.",
                ),
            briefingShort =
                listOf(
                    "Snow quality (depart-des-pistes): On the flat (1087 m): wet heavy snow between 10:00 and 15:00.",
                    "Sunset 17:02.",
                ),
            bestTime = "Best start around 12:30 (back around 16:06): wet heavy snow, then transformed.",
            wax = "Morning 9:00–12:00, air +5 °C, snow ≈ 0 °C, wet snow: classic, uncertain grip, waxless skis advised.",
            loops = loopsEn,
            notes = listOf("snow map: not in the pack"),
            group = "2 adults, children of 10 and 14",
            neverSafe =
                "This plan is never a guarantee: information is indicative, read the BERA and the official " +
                    "sources, and decide on the spot.",
        )

    fun ready(
        morning: MorningView? = this.morning,
        computing: Boolean = false,
        choice: Int = 0,
    ) = DayUi.Ready(
        siteId = "chapelle-des-bois",
        siteName = "Chapelle-des-Bois",
        starts = listOf(LAUS),
        inputs = DayInputs(LocalDate.of(2026, 11, 16), LAUS, 120, false, "16:00", false),
        morning = morning,
        computing = computing,
        choice = choice,
        groups = groups,
        group = groups[0],
        contact = Contact("Marie", "06 12 34 56 78"),
    )

    val plan =
        DayPlanView(
            "/data/user/0/fr.skimap.app/files/plans/2026-11-16-1",
            "chapelle-des-bois",
            "Chaumoz – La Norbière – Les Mortes",
            "le départ des pistes de Chapelle-des-Bois",
            "10 h 00",
            1_794_819_600_000,
            12.8,
            "13 h 55",
            1_794_833_700_000,
            "16 h 00",
            1_794_841_200_000,
            "14 h 40",
            "Temps estimé pour 2 adultes, enfants de 6 et 9 ans (a priori, aucune sortie apprise) : 2 h 35 à 6 h 10",
            listOf(
                KeyPointView("départ", "le départ des pistes de Chapelle-des-Bois", 0.0, "10 h 00"),
                KeyPointView("carrefour", "Chaumoz", 2.1, "10 h 25"),
                KeyPointView("refuge", "Chalet de la Frasse", 6.4, "11 h 20"),
                KeyPointView("arrivée", "le départ des pistes de Chapelle-des-Bois", 12.8, "12 h 25"),
            ),
            listOf("Au carrefour de Chaumoz, retour direct : 25 min de moins"),
            listOf("Skis à écailles — retenue incertaine"),
            "Plan du lundi 16 novembre…",
            "Sortie ski de fond chapelle-des-bois le 16/11 : départ à 10 h 00, boucle Chaumoz – La Norbière – Les Mortes (12,8 km). " +
                "Retour prévu vers 13 h 55, au plus tard 16 h 00. Sans nouvelles de moi à 16 h 00, appelle le 112 et donne ce plan.",
            listOf(6.1125, 46.6032, 6.12, 46.61),
        )

    fun guidance(
        phase: Phase = Phase.MOVING,
        proposal: Proposal? = null,
        route: String = "plan",
    ) = Guidance(
        phase = phase,
        route = route,
        label = "Chaumoz – La Norbière – Les Mortes",
        remainingM = 8_420.0,
        lengthM = 12_800.0,
        progressM = 4_380.0,
        nextTurn = Turn.LEFT,
        nextDistanceM = 243.0,
        nextName = "Chaumoz",
        remainingS = 3_900.0,
        remainingText = "1 h 05",
        arrival = "12 h 40",
        arrivalMs = 0,
        arrivalRange = "12 h 20 à 13 h 15",
        limit = "16 h 00",
        offRouteM = 4.0,
        gpsMode = GpsMode.POCKET,
        proposal = proposal,
        switch = null,
    )

    val proposal =
        Proposal(
            1,
            "short_loop",
            "Vous allez 50 % moins vite que prévu, avec plus de pauses. Je propose la boucle courte, retour 13 h 10 (13 h 00 à 13 h 25), " +
                "45 min de moins. Accepter ou refuser.",
        )
}
