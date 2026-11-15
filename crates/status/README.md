# skimap-status

Instantané de l'état des pistes (pipeline), interrogé une fois pour tous les utilisateurs. Ticket : P13 (#26).

`skimap_status::poll(site, &layout, &downloader, personal_sources)` interroge les sources du site, enregistre un instantané horodaté et écrit `data/out/<site>/status/` :

| Fichier | Contenu |
|---|---|
| `status.json` | dernier contenu normalisé de chaque source du site |
| `status.geojson` | itinéraires en WGS84 avec `itinerary` (identifiant lu par l'assistant : `enj:<id>`, `nordicfrance:<mots du nom>`, `cervieres-mairie:<mots du nom>`), `source`, `name`, `state` (`open`, `closed`, `unknown`), `polled_at`. ENJ : géométrie de l'API, `id`, `route`, `notes`, `update_datetime`, `practice`, `difficulty`, `length_m`, `gtj`, lien `gpx`. Cervières : `MultiLineString` des voies OSM de la couche `pistes` rattachées par le nom (voir plus bas), `sector`, `activity`, `practice` (`nordic`, `foot`, `any`), `state_label`, `length_km` publié, `flag`, `comment`, `mairie` (ligne de la page de la mairie : état, damage, classique, skating, commentaire), `conflict`, `osm_way_ids`, `osm_length_m` |
| `<source>/polls.jsonl` | une ligne par interrogation : heure, origine (`downloaded`, `not_modified`, `cache`), changement ou non, instantané |
| `<source>/snapshots/AAAAMMJJTHHMMSSZ.json` | contenu normalisé, écrit seulement quand il change |
| `<source>/history.json` | fréquence observée : nombre d'interrogations, durée couverte, contenus distincts, heures des changements, intervalle moyen, dates publiées par la source et leur première observation ; pour l'ENJ, transitions d'état par itinéraire et heures de la journée des notes d'état |
| `status.meta.json` | sources (dont OpenSnowMap pour la géométrie OSM à Cervières), licences, attributions ; `stats.join` à Cervières : pistes du widget rattachées ou non, lignes de la mairie liées au widget, à OSM seul ou à rien, désaccords d'état |

## Géométrie des pistes de Cervières

Ni le widget Nordic France ni la page de la mairie ne donnent de géométrie : jusqu'au 15/11/2026, `status.geojson` n'existait qu'à Chapelle-des-Bois, et le pack de Cervières n'avait pas d'état par piste. `poll` lit maintenant la couche `pistes` (P2, à construire avant) et rattache chaque nom (`skimap_status::join`) :

- une piste du widget prend les voies OSM de l'emprise dont une **relation** porte son nom (`skimap_common::names::same_name` : mêmes mots sans articles, accents, longueurs ni mots génériques, ou tous les mots du nom le plus court dans l'autre) ; seulement si aucune relation ne correspond, les voies dont le nom propre correspond (le nom d'une voie est souvent celui de la route, « Route du Col d'Izoard », qui continue au-delà de la piste, côté Arvieux) ;
- les voies doivent convenir à la pratique de la ligne d'activité : ski (`uses` contient `nordic`) pour « Ski de fond », marche (`hike`) pour « Raquettes », « Marche/trail », « Fatbike », toutes pour les deux ;
- une ligne de la mairie va à la piste du widget dont le nom est le plus proche (`names::name_score`) ; une ligne sans piste du widget est rattachée à OSM elle-même ;
- les noms sans voie sont listés dans `stats.join`, jamais devinés.

Le 15/11/2026 : 8 pistes du widget sur 12 rattachées (sans voie OSM : « Liaison les Aittes par la carrière », « Liaison les Aittes par la route », « Piéton les Fonts », « Piéton les Oules ») ; 9 lignes de la mairie sur 10 liées au widget, « Liaison Cervières-le Laus 1.5m » liée à rien ; aucun désaccord d'état. Longueurs OSM rattachées comparées aux longueurs publiées : Gibera 1,42 / 1,5 km, Savare 2,23 / 2,5, Cancel 4,22 / 3,5, Pinatelle 7,83 / 7,5, Aigalan 5,77 / 8, Sioux 12,96 / 6 (14 km sur la page de la mairie), Izoard 7,83 / 16 (la relation OSM « Izoard » s'arrête au col, à 44,820° N).

## Sources

| Site | Source | Accès | Fréquence |
|---|---|---|---|
| Chapelle-des-Bois | API Geotrek de l'ENJ, `trek/?cities=14&language=fr&fields=…` (champ `route` : 1 Fermé, 2 Ouvert ; `labels` « Marquée … le JJ/MM/AAAA à HHhMM » ; `update_datetime`), géométries une fois par jour | sans clé ; ni `ETag` ni `Last-Modified`, donc la liste (9 ko avec `fields=`) est téléchargée à chaque fois | toutes les 15 min (durée de cache de la source `enj`) |
| Cervières | widget Nordic France `/widget/marqueblanche/station/3167` : secteurs, date de damage par secteur, état et commentaire par piste, « Mise à jour » | HTML seulement | au plus toutes les 3 h |
| Cervières | page « Info damage » de la mairie : un tableau par secteur (piste, ouvert/fermé, damage, classique, skating, commentaire), `article:modified_time` | HTML seulement | au plus toutes les 3 h |

**Usage personnel** : aucune de ces sources n'affiche de licence (ENJ : ticket L1 ouvert ; Nordic France et mairie : HTML sans conditions). `poll` refuse donc de tourner sans `personal_sources` (erreur `PersonalSourcesDisabled`, rien n'est écrit). L'option est `skimap_common::config::Config::personal_sources`, lue dans `SKIMAP_PERSONAL_SOURCES=1` (ou `true`).

## Planification

Le pipeline appelle `poll` à intervalle fixe, par exemple toutes les 15 minutes (cron ou minuterie systemd) :

```sh
*/15 * * * * cd /chemin/skimap && SKIMAP_PERSONAL_SOURCES=1 target/release/examples/poll chapelle-des-bois
0 */3 * * *  cd /chemin/skimap && SKIMAP_PERSONAL_SOURCES=1 target/release/examples/poll cervieres
```

Une semaine de ces instantanés en saison donne la fréquence réelle de mise à jour de chaque source (`history.json`).
