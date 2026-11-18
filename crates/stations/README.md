# skimap-stations

Fiches pratiques des stations nordiques (H1, #132), titres de redevance et zones de validité (H2, #133), pour les 219 domaines du recensement (P3, `data/out/france/domaines/`), **rafraîchis depuis leurs sources** par une tâche planifiée.

API : `refresh(&layout, &downloader, personal_sources, now, force)` (tâche planifiée du serveur, idempotente) ; `build(&layout, &downloader, personal_sources)` (tout, tout de suite). Exemple : `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-stations --example stations`.

## Sources (relues à chaque rafraîchissement)

| Source | Accès | Licence | Ce qu'on garde |
|---|---|---|---|
| OpenStreetMap | Extraits Geofabrik locaux (`skimap_osm::local`, [X5](../osm/docs/local.md) ; autrefois Overpass) ; requêtes « loin » (secours en montagne, ambulances, centres de secours, offices de tourisme, écoles de ski : emprises + 6 km) et « près » (emprises + 1,3 km, 6 petites boîtes par requête : premiers secours, postes de pisteurs, défibrillateurs, téléphones d'urgence, points de rencontre des secours, vente, parkings, recherche par nom) ; relations `site=piste` | ODbL | catégorie, nom, téléphones, site, horaires, capacité |
| OpenSkiMap `ski_areas.csv`, SkiRadar | mêmes requêtes que le recensement | ODbL ; « réutilisables avec attribution « skiradar.fr » » | sites de la station, école de ski, fiche SkiRadar |
| Nordic France, ENJ, Haute-Savoie Nordic, Nordic Alpes du Sud, Nordic Isère | pages lues et **analysées** à chaque fois (`sources.rs`) | aucune licence : **usage personnel** (`personal_sources`) | titres, validité citée, prix datés, listes de membres, groupes de sites de la boutique ENJ, pages de domaine (prix journée, téléphones) |

Rattachement d'un point OSM : à moins de 1 km de l'emprise (poste de secours, point de vente), 3 km (secours en montagne, ambulance, centre de secours), 500 m (défibrillateur), 200 m (téléphone d'urgence), 100 m (point de rencontre), 300 m (parking) ; offices de tourisme et écoles de ski : les 2 plus proches à moins de 6 km.

Membres → domaines : mots de chaque partie du libellé dans le nom, le nom OpenSkiMap ou la commune d'un domaine du département ; sinon nom complet unique en France. Compléments manuels **minimaux** (`catalogue.rs`) : 26 rattachements qu'aucune source ne nomme (Agy = Saint-Sigismond, Lacs et Montagnes du Haut-Doubs…) et les prix de la grille PDF de l'ENJ (pas de lecteur PDF), chacun avec sa raison, sa date et une **date d'expiration** ; le test `complements_not_expired` échoue après expiration ; la grille PDF est épinglée par son SHA-256 : si elle change, ses prix sont retirés et la source signalée.

## Rafraîchissement

- Fiches : chaque semaine. Titres, prix, membres : chaque jour du 1er novembre au 30 avril, chaque semaine sinon. Une deuxième exécution le même jour ne refait rien.
- `journal.jsonl` : changements entre deux passages (fiche, titre, page de domaine, état d'une source) ; chaque fiche et chaque titre portent `checked_at` et `changed_at`, chaque prix son `checked_at`.
- `status.json` : état de chaque partie et de chaque source (`ok`, `unreachable`, `broken` : la page ne contient plus ce que l'analyseur attend, `changed` : PDF modifié). Un titre dont la source échoue garde ses valeurs marquées `stale` ; après 14 jours ses prix sont retirés (`perime`) plutôt qu'affichés faux.

## Sorties (`data/out/france/stations/`, WGS84)

- `stations.geojson` : une fiche par domaine : `secours.numbers` (**112 toujours en premier**, puis 15 et 114), `secours.posts`, `vente`, `links`, `phones`, `operator`, `offices_tourisme`, `ecoles_ski`, `parkings`, `has`, `checked_at`, `changed_at` (ODbL, attribution SkiRadar). `pois.geojson` : les points OSM et leurs domaines.
- Avec `personal_sources` seulement (sinon supprimés) : `titres.json` (titres, prix, membres, pages de domaine, documents, compléments), `titres.geojson` (emprise de chaque titre), `couverture.json` (titres de chaque domaine, règle et prix du titre de site, ways OSM de chaque titre).
- `status.json`, `journal.jsonl`, `refresh_state.json`, `stations.meta.json`.

## Chiffres au 15/11/2026 (219 domaines)

- 112 en tête des 219 fiches ; poste de secours (pisteurs, premiers secours, secours en montagne) : 64 (29 %) ; avec centres de secours et défibrillateurs : 164 (75 %).
- Point de vente OSM : 120 (55 %) ; téléphone : 123 (56 %) ; site : 175 (80 %) ; parking : 208 (95 %).
- Titre au prix daté (usage personnel) : 145 (66 %) ; prix journée : 1 (jeu de test alpin). Poste de secours, point de vente et prix daté ensemble : 38 (17 %).
- Titres : Pass national 143 domaines, Montagnes du Jura 39, NP 74 : 19, Isère-Drôme 22, Alpes du Sud 25, 9 Pass de site ENJ ; 10 sources et 32 pages de domaine lues sans erreur.
- Overpass : 98 requêtes (42 « loin », 56 « près ») ; avec les deux instances surchargées le 15/11/2026, environ 2 h au premier passage ; ensuite le cache (1 jour) et la période hebdomadaire.

## Jura (hypothèse de l'utilisateur)

Vraie pour les Pass saison et hebdo Montagnes du Jura, fausse pour la journée : « Le Pass séance est valable uniquement sur le site d'achat du Pass. Une réciprocité existe quand plusieurs domaines peuvent être reliés ski aux pieds. » (FAQ de l'ENJ, lue à chaque rafraîchissement). 39 des 48 domaines de l'Ain, du Doubs et du Jura sont membres de l'ENJ.

## Limites

- Points de vente : seulement ceux qu'OSM décrit.
- Prix journée : seulement quand la page de domaine écrit la saison (jeu de test alpin, hiver 2025-2026) ; un prix sans saison n'est jamais publié.
- Pas de titre régional relevé pour la Savoie, le Massif central, l'Alsace, les Vosges, les Pyrénées (pages sans tarif lisible) : seul le Pass national les couvre.
