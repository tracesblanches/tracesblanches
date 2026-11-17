# X5 : extraits OSM locaux à la place d'Overpass (#139)

[← skimap-osm](../README.md)

Le 15/11/2026, overpass-api.de refusait les connexions ou répondait 502 pendant des heures : 66 sites sur 139 de la construction de toute la France ont échoué à l'étape `refuges`. Le pipeline ne dépend plus d'Overpass : `skimap_osm::local` lit des extraits Geofabrik `.osm.pbf` en Rust (crate `osmpbf`).

## Extraits

- Téléchargés par le téléchargeur commun (source `geofabrik` : 1 requête / 10 s, cache 7 jours puis requête conditionnelle `ETag` / `Last-Modified`, délai 3 h). Si Geofabrik ne répond pas, la copie du cache sert, quel que soit son âge.
- Choisis d'après le registre des sites : le catalogue Geofabrik `index-v1.json` donne le polygone de chaque extrait ; un extrait est lu si un point d'une emprise de site (agrandie de 0,06°, grille de 11 × 11 points) est dans son polygone. Candidats : les régions de France et les pays voisins (Suisse, Italie nord-ouest, Andorre, Catalogne, Aragon, Navarre, Pays basque, Bade-Wurtemberg, Sarre, Rhénanie-Palatinat, Belgique, Luxembourg, Monaco, Liechtenstein).
- Au 15/11/2026 (272 sites) : 19 extraits, 4,9 Go (Rhône-Alpes 533 Mo, Provence-Alpes-Côte d'Azur 391 Mo, Franche-Comté 125 Mo, Suisse 551 Mo, Italie nord-ouest 595 Mo, Belgique 699 Mo…), au lieu de la France entière (5,1 Go) plus les voisins : les emprises débordent en Suisse (30 sites), en Italie (10), en Belgique (5), en Espagne (Navarre, Aragon, Catalogne). Aucun point d'emprise hors de ces polygones.
- Premier téléchargement ≈ 10 min (≈ 10 Mo/s), ensuite revalidation (304).

## Index

Une lecture des extraits (3 passes parallèles : éléments retenus, nœuds et chemins membres, nœuds des chemins membres ; 0,2 à 4,3 s par extrait, 27,7 s pour les 19) garde les éléments que demandaient les anciennes requêtes Overpass, toutes leurs étiquettes et leur boîte englobante :

| Filtre | Ancienne requête | Utilisé par |
|---|---|---|
| `is_hut` | `tourism~^(alpine_hut\|wilderness_hut)$`, `amenity=shelter`, `ref:refuges.info` | `skimap-refuges` |
| `is_station_far` | secours en montagne, ambulances, pompiers, offices de tourisme, écoles de ski | `skimap-stations` (points des stations) |
| `is_station_near` | premiers secours, défibrillateurs, téléphones et points de rencontre des secours, parkings, billetteries, recherche par nom (`foyer`, `caisse`, `^ESF`…) | `skimap-stations` |
| `is_site_relation` | `rel(id:…)` des relations `site=piste` | `skimap-stations` (contacts) |

L'index (`cache/osm-local/index-v1-<clé>.jsonl.gz`, 928 823 éléments, 43 Mo) est écrit atomiquement (fichier temporaire, `fsync`, renommage) ; sa clé est la version du format et le sha256 de chaque extrait : un extrait changé le reconstruit. Un index tronqué (nombre d'éléments différent de l'en-tête) est reconstruit ; un extrait illisible est retéléchargé une fois. Relu en 0,8 s, une fois par processus (`build-all` le partage entre tous les sites), revalidé au plus toutes les 6 h. Les index inutilisés depuis 14 jours sont supprimés (plusieurs dossiers de données peuvent partager le cache, chacun avec son registre).

Une requête par emprise donne la même réponse JSON qu'Overpass `out center tags` (centre de la boîte englobante, 7 décimales) ; les analyseurs des couches sont inchangés.

Écarts par construction : un chemin ou une relation est dans une emprise si sa boîte englobante la rencontre (Overpass : si un de ses segments la rencontre ; les éléments gardés sont des bâtiments et petites surfaces) ; la boîte d'une relation est celle de ses nœuds et chemins membres (les sous-relations ne sont pas suivies) ; les relations `site=piste` n'ont que leurs étiquettes.

## Mêmes résultats qu'avec Overpass

Comparaison avec les sorties construites par Overpass le 15/11/2026 à 01 h 11 et 01 h 37 UTC (extraits Geofabrik : données du 14/11/2026 à 21 h 20 UTC) :

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| éléments OSM (`osm.geojson`) | 20 / 20, mêmes identifiants | 73 / 73, mêmes identifiants |
| propriétés (étiquettes, type, `ref:refuges.info`, téléphones, `in_france`) | identiques | identiques |
| position | écart maximal 1,4 cm (2 chemins à plus de 1 cm : arrondi du centre d'Overpass) | écart maximal 1,3 cm (10 chemins) |
| `links.json` | 9 liens, identiques | 57 liens, identiques |
| fiches `refuges.geojson` | 12 / 12 | 63 / 63 |

Couche `stations` de la France (219 domaines) : 3 549 points d'intérêt avec Overpass, 3 549 avec l'index, tous les mêmes ; tous les chiffres du méta identiques (secours, ventes, parkings, téléphones, sites web), 166 requêtes locales au lieu des requêtes Overpass.

## Ce qui reste sur Overpass

`crates/pisteplan/src/osmref.rs` (repères des plans des pistes, usage personnel, jamais construit sur le serveur ; secours IGN déjà en place) ; `crates/avalanche/src/historic` lit un extrait Overpass sauvegardé (sans réseau) ; les chaînes de provenance de `crates/meteo/src/hourly.rs`.
