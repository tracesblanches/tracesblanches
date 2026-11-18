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

## Index des repères des plans des pistes (18/11/2026)

Les plans des pistes (`skimap-pisteplan`, P22) se calent sur des repères nommés : lieux, sommets, cols, sources, refuges, abris, musées, hôtels, restaurants, lieux de culte, plans d'eau, objets historiques. Ils étaient demandés à Overpass (instance principale, puis miroir VK Maps), injoignable du 15 au 18/11/2026 (502 « at capacity », 504). Le filtre `is_landmark` reprend l'ancienne requête à l'identique : nom obligatoire ; `place` sur les nœuds et chemins ; `natural=peak|saddle|volcano|spring` et `mountain_pass=yes` sur les nœuds seulement ; le reste sur tous les types.

Ces repères ont leur propre index (`Set::Landmarks`, `cache/osm-local/landmarks-v1-<clé>.jsonl.gz`) : 1 353 174 éléments, 76 Mo, construit en 76 s pour les 19 extraits (machine chargée, charge 110 sur 20 cœurs), relu en 2 à 4 s, lu seulement par les plans des pistes. Les mettre dans l'index des couches l'aurait fait passer de 0,9 s à 7 à 9 s de lecture dans chaque processus de `build-all` et de `skimap refresh` ; l'index des couches garde sa clé et n'est pas reconstruit (relu en 0,9 s, 928 823 éléments).

**Mêmes éléments qu'Overpass.** Les 13 réponses Overpass des repères présentes dans le cache (15 et 17/11/2026, 5 massifs : Beaufortain–Mont-Blanc, Maurienne–Vanoise, Briançonnais–Queyras, Jura, Vosges), comparées à l'index sur la même emprise (emprise des nœuds de la réponse, réduite de 0,005°) : 17 068 éléments des deux côtés, les mêmes identifiants, positions à 1 cm près. 4 éléments ont des étiquettes différentes : 3 `wikidata` ajoutés le 15/11/2026 à 00 h 48 UTC et un `addr:country` retiré le 15/11/2026 à 02 h 04 UTC, après la date des extraits (14/11/2026 21 h 20 UTC) ; l'historique de ces nœuds sur l'API OSM le confirme. Trois autres massifs, sans réponse Overpass en cache, sont comparés à l'API OSM (`map`, même filtre écrit à part en Python) : Pyrénées (Plateau de Beille, 61 / 61), Massif central (Le Lioran, 33 / 33), Vercors (Autrans, 46 / 46), sans aucune différence.

**Ordre des sources inchangé.** Les recalages prenaient, dans l'ordre : une réponse Overpass déjà en cache (lue hors ligne), sinon la toponymie IGN (BD TOPO), sinon Overpass sur le réseau. Seule cette dernière étape change : elle lit l'index local. Sur quatre sites (Plateau de Retord, Les Rousses, Autrans, le jeu de test alpin), les sorties `plan_pistes.geojson`, `plan_landmarks.geojson` et `plan_ways.geojson` sont identiques octet pour octet à celles de main (même plan, mêmes pistes exactes : 14, 20, 0, 3). Mettre les repères OSM avant l'IGN changerait les recalages (mesuré : 14 → 16 pistes exactes à Retord, 20 → 11 aux Rousses, 3 → 2 sur le jeu de test alpin) : choix de qualité laissé ouvert, hors de ce ticket.

## Ce qui reste sur Overpass

Aucune requête réseau : le test `crates/osm/tests/no_overpass.rs` échoue si une crate fait une requête sur la source `overpass` qui n'est pas une lecture hors ligne du cache, ou nomme un miroir. Restent sans réseau : les réponses Overpass déjà en cache relues par les plans des pistes (clé de cache : `osmref::landmark_query`), `crates/avalanche/src/historic`, qui lit un extrait Overpass sauvegardé, et les chaînes de provenance de `crates/meteo/src/hourly.rs`.
