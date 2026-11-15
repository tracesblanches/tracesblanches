# skimap-assist

Assistant : briefing, fartage, qualité de neige, affluence, suggestions, équipement, synthèse, alertes, meilleur moment, bilan, questions. Tickets : A1 (#88) à A11 (#98).

Côté téléphone : la crate ne télécharge rien, elle lit les couches de `data/out/<site>/`. `cargo tree -p skimap-assist -e normal` ne contient ni `reqwest`, ni `rustls`, ni `hyper` ; `skimap-common` y a les fonctions `default` et `geotiff` (lecture des cartes d'enneigement, `tiff` sans réseau). Les dépendances de développement (`skimap-meteo`, `skimap-snow`) ne servent qu'à l'exemple `history`, côté poste.

Jamais « sans danger » : chaque sortie dit ce qu'elle ne sait pas (couche absente, BERA non lisible, état des pistes inconnu, neige hors carte) et renvoie aux sources officielles.

## Modules

| Module | Ticket | Rôle |
|---|---|---|
| `layers` | — | lecteurs typés des couches : météo horaire (P19, plusieurs fichiers fusionnés), BERA (P8), état des pistes (P13 : ENJ, Nordic France, mairie), domaines (P3), pistes (P2), carte d'enneigement (P6), avalanches historiques (P20), zones sans réseau (P12), zones et règles (P9, P10), itinéraire prévu |
| `inputs` | — | `SiteData::load` : toutes les couches d'un site, avec remplacement des fichiers datés (`Paths`) pour rejouer un jour passé |
| `sun` | A3 | rayonnement de ciel clair sur une pente, nuages ; position du soleil (`skimap_core::ephemeris::sun_position`, formules de l'Astronomical Almanac, écart à astral 3.2 < 0,01°), heure légale et lever / coucher : `skimap_core::ephemeris` |
| `snow_quality` | A3 | qualité de neige heure par heure par classe de pente et d'orientation, meilleur créneau, meilleur versant |
| `wax` | A2 | fartage classique (retenue et glisse) et skating (glisse), tables Swix et Toko citées |
| `briefing` | A1 | classement des domaines et des itinéraires du jour et des jours suivants, chaque raison avec ses points |
| `synthesis` | A7 | trois phrases : avalanche, météo, règles et réseau sur l'itinéraire |
| `crowd` | A4 | affluence (fait, utilisé par A1 et A9) |
| `suggest` | A5 | boucles pour un temps, une technique, un niveau et une heure de retour, sur le graphe C1 avec le temps C5 |
| `equipment` | A6 | liste d'équipement d'une sortie, chaque élément avec sa raison |
| `alerts` | A8 | alertes de changement de conditions et de demi-tour avancé pendant la sortie, rejeux |
| `best_time` | A9 | meilleur moment pour partir |
| `layer` | — | `skimap_assist::build(site, &layout, &Options)` écrit la couche `assist` |

## Sorties (`data/out/<site>/assist/`)

| Fichier | Contenu |
|---|---|
| `briefing_<date>.json` / `.txt` | par jour (le jour demandé et les 3 suivants par défaut) : domaines puis itinéraires classés, chaque raison (`factor`, `points`, `text`), affluence, qualité de neige, coucher du soleil, notes sur les données, lignes vocales (`Event::Briefing` de `skimap-voice`) |
| `snow_quality_<date>.json` / `.txt` | par jour et par point météo : état de surface heure par heure (06 h à 20 h) des 17 classes, énergie, eau liquide, croûte, neige fraîche ; meilleur créneau par classe, meilleure classe ; tableau lisible dans le `.txt` |
| `wax_<date>.json` / `.txt` | par jour et par point : matin (9–12 h) et après-midi (12–16 h), conditions, fart de retenue Swix et équivalent Toko, conseil écailles / peaux, glisse Swix (température de l'air) et Toko (température de la neige), raisons, pages des fabricants |
| `synthesis_<date>.json` / `.txt` | par jour : les trois phrases, les événements vocaux, les faits (niveau BERA, km en zone CLPA, km sans réseau, règles actives avec leur source) |
| `suggestions_<date>.json` / `.txt` / `.geojson` | boucles suggérées par demande, raisons, géométrie pour le visualiseur (A5) |
| `equipment_<date>.json` / `.txt` | liste par sortie (A6) |
| `alerts_<date>.json` / `.txt` | rejeux météo par jour et rejeux de traces sur les boucles (A8) |
| `best_time_<date>.json` / `.txt` | meilleur départ par jour et par point, créneaux essayés (A9) |
| `assist.meta.json` | sources de toutes les couches lues (avec `personal_use_only`), tables de fartage, fichiers, entrées de chaque exécution (`parameters.runs`) |
| `history/` | entrées archivées des jours passés (exemple `history`) : `meteo_<début>_<fin>.json` (format P19 plus `relative_humidity_pct`), `snow_<date>/` (P6), `bera_<date>.json` (P8) |

Chaque JSON commence par `inputs` (fichiers lus) et `missing` (couches absentes).

## Commandes

```sh
# entrées des jours passés (poste ; Open-Meteo est réservé à l'usage non commercial)
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-assist --example history -- cervieres 2026-01-04 2026-01-10
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-assist --example history -- cervieres 2023-04-15 2023-04-15
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-assist --example history -- chapelle-des-bois 2026-02-22 2026-02-28
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-assist --example history -- cervieres 2026-11-15 2026-11-15 --no-snow
# l'assistant (aucun réseau)
cargo run --release -p skimap-assist --example assist -- cervieres 2026-01-05 --days 5 --history past \
    --home 6.6426,44.8986 --route data/out/cervieres/routes/routes.geojson --suggest --outings
cargo run --release -p skimap-assist --example assist -- chapelle-des-bois 2026-11-15 --history with-forecast \
    --home 6.3554,46.9035 --route data/out/chapelle-des-bois/routes/offpiste_risoux_fevrier.geojson
cargo test -p skimap-assist --test real_sites -- --ignored   # contrôles sur les données réelles
```

`--history past` lit les fichiers de `history/` (chaque jour prend sa carte d'enneigement et son BERA) ; `with-forecast` ajoute la prévision courante (`meteo-hourly`) et la carte d'enneigement courante. Le point de départ « maison » des exemples est Briançon (Cervières) et Pontarlier (Chapelle-des-Bois), choisis comme villes d'accès, pas comme domicile de quelqu'un.

Temps de calcul (`build`, release, 20 cœurs) : 0,15 à 0,23 s pour Cervières, 0,29 à 0,46 s pour Chapelle-des-Bois (7 jours) ; la lecture des couches (trois GeoTIFF par jour, zones, 2 740 polygones sans réseau) domine.

## Détail par ticket

- [A1 : briefing du jour](docs/briefing.md) : candidats, barème, voix, ce que A5 et A9 réutilisent
- [A2 : fartage](docs/fartage.md) : tables Swix et Toko citées, règles, écailles et peaux
- [A3 : qualité de neige](docs/qualite-neige.md) : heuristique, validation qualitative sur des jours passés
- [A7 : synthèse](docs/synthese.md) : les trois phrases, la voix
- [A5 : suggestion d'itinéraire](docs/suggestions.md) : boucles, barème, limites du graphe
- [A6, A8, A9 : équipement, alertes, meilleur moment](docs/equipement-alertes-depart.md)
- [Limites](docs/limites.md)
