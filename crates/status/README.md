# skimap-status

Instantané de l'état des pistes (pipeline), interrogé une fois pour tous les utilisateurs. Ticket : P13 (#26).

`skimap_status::poll(site, &layout, &downloader, personal_sources)` interroge les sources du site, enregistre un instantané horodaté et écrit `data/out/<site>/status/` :

| Fichier | Contenu |
|---|---|
| `status.json` | dernier contenu normalisé de chaque source du site |
| `status.geojson` | itinéraires en WGS84 avec `osm_way_ids` (voies OSM de la couche `pistes` que l'itinéraire couvre, toutes sources), `osm_practice` (`nordic`, `foot`, `any`), `state_at` (date de l'état publiée par la source : `update_datetime` ENJ, « Mise à jour » du widget datée avec l'année du relevé, date de la page de la mairie), `itinerary` (identifiant lu par l'assistant : `enj:<id>`, `nordicfrance:<mots du nom>`, `mairie:<mots du nom>`), `source`, `name`, `state` (`open`, `closed`, `unknown`), `polled_at`. ENJ : géométrie de l'API, `id`, `route`, `notes`, `update_datetime`, `practice`, `difficulty`, `length_m`, `gtj`, lien `gpx`. Jeu de test alpin : `MultiLineString` des voies OSM de la couche `pistes` rattachées par le nom (voir plus bas), `sector`, `activity`, `practice` (`nordic`, `foot`, `any`), `state_label`, `length_km` publié, `flag`, `comment`, `mairie` (ligne de la page de la mairie : état, damage, classique, skating, commentaire), `conflict`, `osm_way_ids`, `osm_length_m` |
| `<source>/polls.jsonl` | une ligne par interrogation : heure, origine (`downloaded`, `not_modified`, `cache`), changement ou non, instantané |
| `<source>/snapshots/AAAAMMJJTHHMMSSZ.json` | contenu normalisé, écrit seulement quand il change |
| `<source>/history.json` | fréquence observée : nombre d'interrogations, durée couverte, contenus distincts, heures des changements, intervalle moyen, dates publiées par la source et leur première observation ; pour l'ENJ, transitions d'état par itinéraire et heures de la journée des notes d'état |
| `status.meta.json` | sources (dont OpenSnowMap pour la géométrie OSM sur le jeu de test alpin), licences, attributions ; `stats.join` sur le jeu de test alpin : pistes du widget rattachées ou non, lignes de la mairie liées au widget, à OSM seul ou à rien, désaccords d'état |

## Géométrie des pistes du jeu de test alpin

Ni le widget Nordic France ni la page de la mairie ne donnent de géométrie : jusqu'au 15/11/2026, `status.geojson` n'existait que sur le jeu de test jurassien, et le pack du jeu de test alpin n'avait pas d'état par piste. `poll` lit maintenant la couche `pistes` (P2, à construire avant) et rattache chaque nom (`skimap_status::join`) :

- une piste du widget prend les voies OSM de l'emprise dont une **relation** porte son nom (`skimap_common::names::same_name` : mêmes mots sans articles, accents, longueurs ni mots génériques, ou tous les mots du nom le plus court dans l'autre) ; seulement si aucune relation ne correspond, les voies dont le nom propre correspond (le nom d'une voie est souvent celui de la route, « Route du Col d'Izoard », qui continue au-delà de la piste, côté Arvieux) ;
- les voies doivent convenir à la pratique de la ligne d'activité : ski (`uses` contient `nordic`) pour « Ski de fond », marche (`hike`) pour « Raquettes », « Marche/trail », « Fatbike », toutes pour les deux ;
- une ligne de la mairie va à la piste du widget dont le nom est le plus proche (`names::name_score`) ; une ligne sans piste du widget est rattachée à OSM elle-même ;
- les noms sans voie sont listés dans `stats.join`, jamais devinés.

Le 15/11/2026 : 8 pistes du widget sur 12 rattachées (sans voie OSM : « Liaison les Aittes par la carrière », « Liaison les Aittes par la route », « Piéton les Fonts », « Piéton les Oules ») ; 9 lignes de la mairie sur 10 liées au widget, la liaison vers le Laus (1.5m) liée à rien ; aucun désaccord d'état. Longueurs OSM rattachées comparées aux longueurs publiées : Gibera 1,42 / 1,5 km, Savare 2,23 / 2,5, Cancel 4,22 / 3,5, Pinatelle 7,83 / 7,5, Aigalan 5,77 / 8, Sioux 12,96 / 6 (14 km sur la page de la mairie), Izoard 7,83 / 16 (la relation OSM « Izoard » s'arrête au col, à 44,820° N).

## Voies OSM des itinéraires ENJ

L'ENJ publie sa propre géométrie, dessinée à la main. `join::ItineraryGraphs` l'échantillonne tous les 10 m et l'apparie au graphe de la couche `pistes` (`graph_edges.geojson`) restreint à sa pratique (ski : voies `nordic` ; raquettes : voies `hike` ; multi-activités : toutes ; liaisons comprises, toutes les arêtes à double sens), avec le HMM du rattachement C2 (`skimap_core::matching`, Viterbi). Les voies du chemin appariées sur au moins 30 m (ou la moitié de la voie) à moins de 50 m appartiennent à l'itinéraire (`osm_way_ids`) : le pack joint ainsi l'état à chaque voie de la couche `pistes`. `stats.enj_join` de `status.meta.json` donne par itinéraire les points dans l'emprise, le taux de rattachement, les points atteignables (une voie de la pratique à moins de 50 m) et les écarts d'au moins 50 m avec leur cause (`no_way` : pas de voie OSM, `other_practice` : seulement des voies d'une autre pratique, `unmatched` : voie de la pratique proche mais non retenue). Sans la couche `pistes`, `osm_way_ids` vaut `null`.

La règle précédente (`join::ways_along` : voies dont 80 % des points sont à moins de 20 m de l'itinéraire) refusait toute voie plus longue que le tronçon partagé avec l'itinéraire. Mesure du 17/11/2026 (X12, #146), `cargo run --release -p skimap-status --example enj_join -- <sites>` sur le jeu de test jurassien et quatre autres domaines ENJ du Doubs et du Haut-Jura (Les Fourgs, La Vattay, Les Rousses, Mouthe) (itinéraires traversant l'emprise de chaque site, `trek/?in_bbox=`, points dans l'emprise ; un itinéraire commun à deux emprises compte deux fois) :

| Pratique | Itinéraires | Longueur dans l'emprise | Voie de la pratique à ≤ 50 m | Rattaché (HMM) | dont à ≤ 20 m | Rattaché / atteignable | Règle 80 % / 20 m |
|---|---|---|---|---|---|---|---|
| Ski de fond | 268 | 1 940 km | 93,6 % | 92,7 % | 84,1 % | 99,1 % | 70,7 % |
| Multi-activités | 23 | 113 km | 66,3 % | 64,9 % | 51,5 % | 98,0 % | 30,2 % |
| Raquettes | 155 | 1 003 km | 21,1 % | 20,5 % | 15,8 % | 96,9 % | 8,6 % |
| Toutes | 446 | 3 056 km | 68,8 % | 68,0 % | 60,5 % | 98,8 % | 48,8 % |

Par site, ski de fond (HMM / règle 80 %) : jeu de test jurassien 94,8 / 72,3 %, Les Fourgs 87,1 / 59,4 %, La Vattay 91,2 / 72,7 %, Les Rousses 90,1 / 55,9 %, Mouthe 96,2 / 81,9 %. Sur le vrai pipeline (`poll`, jeu de test jurassien, 42 itinéraires de la commune) : 709 couples (itinéraire, voie) au lieu de 534, ski 94,3 % des points rattachés.

Écarts relus : ce qui reste non rattaché est presque entièrement hors des voies OSM. Ski : 89,6 km d'écarts `no_way` (par exemple Chez Michel, 2,8 km entre 65 et 250 m de toute voie, piste absente d'OSM ; La Bois de Ban 2,9 km ; liaisons vers Oye-et-Pallet et Le Village), 4,1 km `other_practice`, 0,5 km `unmatched`. Multi-activités : 28,9 km `no_way` (pistes Trappeur du jeu de test jurassien, Inter Villages des Fourgs). Raquettes : 432 km `no_way` et 175 km `other_practice` : les itinéraires de raquettes suivent surtout des sentiers que la couche `pistes` ne contient pas ou longent des pistes de ski, que leur fermeture ne doit pas fermer. Les voies rattachées à plus de 20 m en médiane (67 km sur 1 872 km rattachés en ski et multi-activités) ont été relues sur des exemples : tracé ENJ décalé de 35 à 48 m le long de la seule voie de ski proche (La Combe à la Chèvre, voie 204900141), boucle dessinée à main levée autour de la voie (La Chaux Mourant, voie 899475240).

## Sources

| Site | Source | Accès | Fréquence |
|---|---|---|---|
| Jeu de test jurassien | API Geotrek de l'ENJ, `trek/?cities=14&language=fr&fields=…` (champ `route` : 1 Fermé, 2 Ouvert ; `labels` « Marquée … le JJ/MM/AAAA à HHhMM » ; `update_datetime`), géométries une fois par jour | sans clé ; ni `ETag` ni `Last-Modified`, donc la liste (9 ko avec `fields=`) est téléchargée à chaque fois | toutes les 15 min (durée de cache de la source `enj`) |
| Jeu de test alpin | widget Nordic France `/widget/marqueblanche/station/3167` : secteurs, date de damage par secteur, état et commentaire par piste, « Mise à jour » | HTML seulement | au plus toutes les 3 h |
| Jeu de test alpin | page « Info damage » de la mairie : un tableau par secteur (piste, ouvert/fermé, damage, classique, skating, commentaire), `article:modified_time` | HTML seulement | au plus toutes les 3 h |

**Usage personnel** : aucune de ces sources n'affiche de licence (ENJ : ticket L1 ouvert ; Nordic France et mairie : HTML sans conditions). `poll` refuse donc de tourner sans `personal_sources` (erreur `PersonalSourcesDisabled`, rien n'est écrit). L'option est `skimap_common::config::Config::personal_sources`, lue dans `SKIMAP_PERSONAL_SOURCES=1` (ou `true`).

## Planification

Le pipeline appelle `poll` à intervalle fixe, par exemple toutes les 15 minutes (cron ou minuterie systemd) :

```sh
*/15 * * * * cd /chemin/skimap && SKIMAP_PERSONAL_SOURCES=1 target/release/examples/poll <site>
0 */3 * * *  cd /chemin/skimap && SKIMAP_PERSONAL_SOURCES=1 target/release/examples/poll <site>
```

Une semaine de ces instantanés en saison donne la fréquence réelle de mise à jour de chaque source (`history.json`).
