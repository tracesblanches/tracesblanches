# A1 — briefing du jour (#88)

[← skimap-assist](../README.md)

Candidats : les domaines de la couche `domaines` (4 à Cervières, 9 à Chapelle-des-Bois) et les itinéraires des sources d'état (8 Nordic France à Cervières, 31 ENJ hors raquettes à Chapelle-des-Bois). Un itinéraire ENJ prend sa géométrie dans `status.geojson` ; une piste Nordic France prend les ways OSM dont la relation porte le même nom (« Le Sioux » ↔ « Le Chemin du Sioux » : mots significatifs sans articles). Chaque itinéraire va au domaine qui contient le plus de ses points.

Points (barème dans la doc du module) : état du jour (tout fermé −10 ; sinon +3 × part des km ouverts ; « ouvert » sans neige estimée : 0 et « à vérifier »), neige médiane le long des pistes (carte P6 du jour ou des 4 jours avant, plus la neige tombée depuis : < 20 cm −3, 20–40 cm +0,5, ≥ 40 cm +2, inconnue −1,5), neige fraîche, surface (A3), rafales, froid ressenti, brouillard, neige et pluie dans la journée, soleil, affluence (A4), distance à vol d'oiseau (−0,02 par km), km en zone CLPA (−2 si BERA ≥ 3, −0,5 si BERA indisponible), taille du domaine. Le domaine du site est fermé quand A4 dit que la date est hors des dates d'ouverture publiées (Cervières : du 19/12/2026 au 31/03/2027) ; hors de l'année scolaire 2026-27, A4 ne connaît que les week-ends et ses dates d'ouverture ne s'appliquent pas.

Voix : `Event::Briefing` (ouverture, BERA ou absence de bulletin, pistes ouvertes, recommandation, météo, neige, affluence, coucher du soleil).

Pour A5 et A9 : `briefing::day` renvoie les candidats avec leur neige (`SnowStats`), leur météo (`DayWeather`) et leurs raisons ; `snow_quality::DayQuality` donne par heure et par classe la qualité et le meilleur créneau (le point de départ de A9).
