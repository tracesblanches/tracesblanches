# skimap-stations

Fiches pratiques des stations nordiques (H1, #132), titres de redevance et zones de validité (H2, #133). Pipeline, échelle nationale : les 219 domaines du recensement (P3, `data/out/france/domaines/`).

API : `skimap_stations::build(&layout, &downloader, personal_sources)`. Exemple : `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-stations --example stations`.

## Sources

| Source | Accès | Licence | Ce qu'on garde |
|---|---|---|---|
| OpenStreetMap | Overpass public (source `overpass`, 1 requête / 5 s, cache 1 jour) : 42 requêtes, une par groupe de domaines voisins (emprises + 6 km, boîte de 0,7° × 1° au plus) ; relations `site=piste` des domaines | ODbL | secours (`emergency=*`, `amenity=first_aid`, `mountain_rescue`, centres de secours, noms « poste de secours », « pisteur »), vente (`shop=ticket`, foyers, caisses, billetteries nordiques, distributeurs), offices de tourisme, écoles de ski, parkings ; téléphones, sites, horaires |
| OpenSkiMap `ski_areas.csv` | source `openskimap` (même requête que le recensement) | ODbL | `websites` des domaines OpenSkiMap qui recouvrent le domaine |
| SkiRadar geo-feed | source `skiradar` (mêmes requêtes que le recensement) | « réutilisables avec attribution « skiradar.fr » et lien vers la fiche » | site officiel, école de ski (nom, site), lien vers la fiche |
| Nordic France, ENJ, Haute-Savoie Nordic, Nordic Alpes du Sud, Nordic Isère | pages et grilles lues à la main le 15/11/2026 (`catalogue.rs`), relues à chaque construction | aucune licence de réutilisation : **usage personnel** (`personal_sources`) | titres, validité citée, prix datés, listes de domaines membres |

Rattachement d'un point OSM à un domaine : distance à l'emprise du domaine (0 dedans) au plus 1 km pour un poste de secours ou un point de vente, 3 km pour secours en montagne, ambulance, centre de secours, 500 m pour un défibrillateur, 200 m pour un téléphone d'urgence, 100 m pour un point de rencontre des secours, 300 m pour un parking (sauf privé) ; offices de tourisme et écoles de ski : les 2 plus proches à moins de 6 km. Un « foyer » ne compte comme point de vente que si son nom parle de ski, de fond, de nordique, de pistes, de caisse ou de billetterie (pas un foyer rural).

## Sorties (`data/out/france/stations/`, WGS84)

- `stations.geojson` : une fiche par domaine (point représentatif) : `secours.numbers` (**112 toujours en premier**, puis 15 et 114), `secours.posts`, `vente`, `links` (station, école de ski, fiche SkiRadar), `phones`, `operator`, `offices_tourisme`, `ecoles_ski`, `parkings`, `has` ; licence ODbL, attribution SkiRadar.
- `pois.geojson` : les points OSM et leurs domaines (ODbL).
- Avec `personal_sources` seulement (sinon ces fichiers sont supprimés) :
  - `titres.json` : titres (portée, émetteur, validité citée, notes, validité à l'étranger), prix (produit, catégorie, âges, montant, palier prévente / plein tarif, dates, **saison**, source, date de lecture, citation, `confirmed_now`), membres et domaines (confiance `verifie`, `partiel`, `probable`, `auto`), contrôle de chaque source ;
  - `titres.geojson` : emprise de chaque titre (réunion des emprises de ses domaines) ;
  - `couverture.json` : titres valables sur chaque domaine, règle et prix du titre de site, ways OSM de chaque titre (jointure avec `domaines_ways.json` pour colorer les pistes).
- `stations.meta.json` : sources, licences, propriétés, chiffres.

Un prix n'est écrit qu'avec sa saison, sa source et sa date de lecture ; `titres::check_sources` relit chaque source (citation retrouvée dans le HTML, SHA-256 inchangé pour un PDF) et le dit dans `documents[].check` et `prices[].confirmed_now`.

## Titres au 15/11/2026

| Titre | Validité (citée) | Adulte | Jeune |
|---|---|---|---|
| Nordic Pass national (Nordic France) | « valable sur tous les sites nordiques adhérents » ; pas les nocturnes ni la neige de culture avant le 15/12 ; gratuit en Suisse romande | 214 € jusqu'au 15/11, 255 € | 79 €, 94 € (5-15 ans) |
| Pass saison Montagnes du Jura (ENJ) | « accès à tous les sites nordiques de l'Ain, du Doubs et du Jura, ainsi qu'en Suisse Romande » | 112 / 135 / 173 € | 45 / 50 / 70 € |
| Pass hebdo Montagnes du Jura (ENJ) | « valable sur TOUS les sites nordiques du massif du Jura » | 51 € | 33 € |
| Nordic Pass 74 (Haute-Savoie Nordic) | « les 25 domaines nordiques de Haute-Savoie » | 142 € en prévente, 178 € | 44 €, 52 € |
| Pass Isère-Drôme (Nordic Isère) | « 23 sites de l'Isère et de la Drôme » | 160 € (13-31/10), 175 € | 57 €, 62 € |
| Pass saison et semaine Alpes du Sud | domaines de Nordic Alpes du Sud (déduit) | 143 / 205 € ; semaine 77 € | 72 / 87 € ; 49 € |
| Cervières, titre de site (hiver 2025-2026) | le site | journée 15 €, 3 h 11 €, saison 75 € | 8 €, 6 €, 60 € |

**Jura (hypothèse de l'utilisateur)** : vraie pour les titres saison et hebdo Montagnes du Jura (et le Pass national), fausse pour la journée : « Le Pass séance est valable uniquement sur le site d'achat du Pass. Une réciprocité existe quand plusieurs domaines peuvent être reliés ski aux pieds. Cette règle est la même pour les Pass pluri journaliers (de 2 à 5 jours). » (FAQ de l'ENJ, pass.espacenordiquejurassien.com/faq). Les 48 domaines du recensement dans l'Ain, le Doubs et le Jura sont classés à la main : 39 membres de l'ENJ, 9 hors ENJ (`JURA_NOT_MEMBERS`, vérifié par le test réel).

## Limites

- Points de vente : seulement ceux qu'OSM décrit ; offices de tourisme, mairies et commerces qui vendent les pass saison ne sont pas tous étiquetés.
- Prix journée : seulement Cervières (page Nordic Alpes du Sud, hiver 2025-2026) ; les autres sites publient leurs prix dans des pages dynamiques ou des PDF propres à chaque domaine.
- Membres rattachés automatiquement (`auto`) : vérifiés sur la liste imprimée par `tests/real.rs`, pas un par un sur le terrain.
- Savoie Nordic, Montagnes du Massif Central, Alsace, Vosges, Pyrénées : pas de titre régional relevé (pages illisibles ou sans tarif) ; seul le Pass national les couvre.
