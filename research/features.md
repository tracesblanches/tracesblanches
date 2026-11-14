# skimap : fonctionnalités discutées (14/11/2026)

Versions proposées, pas décidées. Statut : « vérifié » = source ou faisabilité vérifiée ; « à tester » = à mesurer sur l'appareil ou le terrain ; « recherche » = en cours ; « idée » = pas encore étudié.

## Principes

- Gratuit ou achat unique, tout inclus : aucune brique avec un coût par utilisateur et par an.
- Le téléphone seul suffit ; la montre (Wear OS) est optionnelle et n'apporte que du confort.
- Jamais « sans danger » : afficher l'âge et la source de chaque info ; ne jamais remplacer le BERA.
- Pas de carte de chaleur publique du hors-piste.
- Langages : Rust pour le pipeline de données (et peut-être un noyau partagé via UniFFI) ; Kotlin pour l'app Android et la montre.

## Carte et données

| # | Fonction | Source | Version | Statut |
|---|---|---|---|---|
| 1 | Carte hiver hors ligne | Plan IGN v2 vectoriel + PMTiles OSM auto-hébergés | v1 | vérifié |
| 2 | Pistes de fond : nom, couleur, classique / skating, sens | OSM (extrait OpenSnowMap) | v1 | vérifié |
| 3 | Pistes ouvertes / fermées | API ENJ (Chapelle) ; Nordic France par convention (Cervières) | v1 Chapelle | vérifié ENJ, licence à confirmer |
| 4 | Recensement et affichage de tous les domaines de fond | OSM / OpenSkiMap, Nordic France, SkiRadar | v1 France | recherche |
| 5 | Carte des pentes > 30° | IGN SLOPES.MOUNTAIN | v1 | vérifié, licence hors ligne à confirmer |
| 6 | Bulletin d'avalanche | BERA Météo-France (Cervières = Queyras) ; Jura : messages MF + SLF | v1 | vérifié |
| 7 | Zones peu exposées aux avalanches (classe ATES) | auto-ATES / Flow-Py sur relief IGN + forêt | v2 | recherche |
| 8 | Carte d'enneigement estimée | Simulation nivologique MF projetée sur le relief + stations + satellite + prévision AROME | v1 (base) | proposition, à valider |
| 9 | Météo heure par heure : vent, froid ressenti, brouillard, isotherme 0 °C | Météo-France AROME | v1 | idée |
| 10 | Zones protégées avec règles saisonnières | INPN WFS, Biodiv'sports (paramètre period), GPU | v1 | vérifié |
| 11 | Règles de bivouac par zone | Saisie manuelle depuis les arrêtés + contours | v2 | vérifié (sources), travail éditorial |
| 12 | Refuges, cabanes, abris, restaurants | refuges.info, OSM, Camptocamp, DATAtourisme | v1 | vérifié |
| 13 | Zones sans aucun réseau (même 112) | ARCEP Mon Réseau Mobile | v1 | calculé pour les 2 sites |
| 14 | Topos SRN agrégés | GTJ SRN, Camptocamp, Skitour, skirandonneenordique.com | v2 | licences à vérifier |

## Planifier la sortie

| # | Fonction | Version | Statut |
|---|---|---|---|
| 15 | Routage sur pistes | v1 | idée |
| 16 | Routage SRN hors piste : évite pentes > 30°, refuse les zones interdites en période, pondéré par la neige | v2 | à tester (coût sur Pixel 8) |
| 17 | Temps estimé (formule) puis personnalisé sur les sorties de l'utilisateur | v1 formule, v2 perso | proposition |
| 18 | Heure de demi-tour avant la nuit | v1 | idée |
| 19 | Plan de sortie partagé + alerte si pas de retour à l'heure | v1 | idée |
| 20 | Prévision de batterie avec réserve pour l'appel d'urgence | v1 | à tester |

## Pendant la sortie

| # | Fonction | Version | Statut |
|---|---|---|---|
| 21 | Trace et « ramène-moi » (retour sur trace guidé) | v1 | idée |
| 22 | Guidage vocal hors ligne | v1 | idée |
| 23 | Alerte d'écart d'itinéraire (rattachement aux pistes / couloir hors piste) | v1 | à tester sous forêt |
| 24 | Alertes : pente > 30°, zone protégée, demi-tour, entrée en zone sans réseau | v1 | idée |
| 25 | Sons gauche / droite / hors piste, vibrations, signal de rassurance | v1 | idée |
| 26 | Navigation au cap par brouillard | v1 | idée |
| 27 | Économie de batterie : écran éteint, GPS adaptatif, baromètre, mode avion | v1 | à tester par grand froid |
| 28 | Commandes vocales hors ligne | v2 | à tester sur Pixel 8 |
| 29 | App montre optionnelle : cap, temps restant, vibrations, enregistrement | v2 | à tester (Health Services CROSS_COUNTRY_SKIING) |

## Collaboratif

| # | Fonction | Version | Statut |
|---|---|---|---|
| 30 | Signalements en un geste (neige, trace faite, obstacle, refuge ouvert / plein), photo facultative | v2 | idée |
| 31 | Confirmation au passage, expiration par type, réputation | v2 | idée |
| 32 | File d'attente hors réseau, heure réelle du constat | v2 | idée |
| 33 | Mesure passive : glisse du jour, traces ouvertes hors piste | v3 | idée, aucune publication trouvée |
| 34 | Corrections OSM des pistes et itinéraires | v2 | idée |

## Sport et social

| # | Fonction | Version | Statut |
|---|---|---|---|
| 35 | Envoi vers Strava (NordicSki / BackcountrySki) et Health Connect | v1 | vérifié (API Strava, écriture autorisée) |
| 36 | Import de l'historique par l'export Strava de l'utilisateur (pas par l'API) | v2 | interprétation juridique à valider |
| 37 | Détection classique / skating / sous-techniques | v3 | littérature vérifiée (Rindal 2017 : 93,9 %), données à collecter |
| 38 | Classements séparés classique / skating, corrigés par la glisse du jour | v3 | idée |
| 39 | Défis : GTJ complète, tous les itinéraires d'un domaine | v2 | idée |
| 40 | Suivi de groupe en SRN | v2 | idée |
