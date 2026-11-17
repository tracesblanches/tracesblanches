# Format du pack (P14, #27)

[← skimap-pack](../README.md)

Dossier `data/out/<site>/pack/`, écrit dans un dossier temporaire puis substitué d'un coup (un lecteur ne voit jamais un pack à moitié écrit) :

```text
manifest.json                   manifeste (format skimap-pack, version 1)
pack.meta.json                  le pack décrit dans le schéma commun common::meta::LayerMeta
basemap/plan-ign.pmtiles        tuiles vectorielles Plan IGN v2 (MVT gzip)
basemap/plan-ign/style.json     style IGN « standard », URL relatives au pack
basemap/plan-ign/sprite/…       PlanIgn(.json|.png|@2x)
basemap/plan-ign/fonts/<pile>/<plage>.pbf
basemap/osm.pmtiles             extrait Protomaps (MVT gzip)
basemap/osm/style.json, sprites/, fonts/
layers/<id>.geojson             couches vectorielles (WGS84, coordonnées arrondies à 1e-6°)
layers/<id>.json                données (règles, BERA, synthèses, graphe de pistes, météo), JSON compact
layers/offpiste-*.skr.gz        rasters du routeur hors piste de core (SKR1, gzip)
layers/slope.pmtiles            polygones des classes de pente (tuiles vectorielles MVT, gzip)
layers/avalanche-exposure.pmtiles  polygones des classes d'exposition aux avalanches (MVT)
layers/meta/<couche>/<couche>.meta.json   copies des métadonnées des couches, JSON compact
layers/snow-*.pmtiles           polygones des classes de neige (MVT)
voice/manifest.json, voice/clips/*.opus, voice/voice.meta.json
```

Les JSON du pipeline (données et métadonnées des couches) sont écrits indentés pour la lecture ; le pack les recopie en JSON compact, mêmes valeurs (`build::copy_data`, #160) : le téléphone ne fait que les analyser. Sur un site alpin (Bessans, run AROME du 17/11/2026 09 h UTC, trois points de prévision) : briefing de l'assistant 614 → 265 ko, qualité de neige 499 → 254 ko, météo heure par heure 143 → 90 ko. Les GeoJSON sont déjà compacts, coordonnées arrondies. Dans un paquet de mise à jour, ces fichiers texte voyagent en plus compressés gzip ([mise-a-jour.md](mise-a-jour.md#paquet-skb)).

À côté du pack, hors de lui : `data/out/<site>/pack.parts.json`, clés des parties lourdes du pack en place (polygones de classes, fonds de carte), lues par le build suivant pour reprendre une partie dont les entrées n'ont pas changé ([parties reprises](../README.md#parties-reprises-du-pack-précédent-164)) ; jamais publié.

`manifest.json` :

| Champ | Contenu |
|---|---|
| `format`, `format_version` | `skimap-pack`, `1` ; le lecteur refuse un autre format ou une version plus récente |
| `pack_version` | heure de construction UTC `AAAAMMJJTHHMMSSZ`, croissante |
| `generator`, `generated_at`, `site` | auteur, date, site (identifiant, nom, département, emprise `[O, S, E, N]`, centre) |
| `site.record` | fiche du registre du site (P163, #163) : genre, emprise de travail, massifs, départs (X9) ; ses `starts` sont dans l'ordre de `site.start_ranking`, le meilleur d'abord : le téléphone propose le premier |
| `site.start_ranking` | départs classés par le planificateur (#166, `build::rank`) : la matinée du téléphone (`skimap_plan::plan::morning_in`, même code, sur les fichiers du pack lus par `view::PackView`) lancée depuis chaque départ pour la sortie par défaut de l'app : `budget_min` (120), `style` (`classic`), `date` (premier jour de la prévision du pack, sinon le jour du build), `start_time` (`10:00`) ; `starts[]` dans l'ordre retenu : `id`, `registry_rank` (place dans le registre, 0 : départ principal de X9), `loops` (boucles proposées, au plus 3, allers-retours compris), `real_loops`, `best_score` (score de la meilleure boucle), `lengths_km`, `network_km` (longueur parcourue une seule fois), `note` (dernière note du planificateur sans boucle) ; ordre : plus de boucles, puis plus de vraies boucles, puis meilleur score, puis ordre du registre ; `note` quand le planificateur n'a pas pu tourner (ordre du registre gardé) ; `compute_ms` (mesuré le 17/11/2026 sur 14 packs de 11 massifs : 4 à 238 ms par pack, une matinée par départ en parallèle, contre 0,5 à 1,4 s de build-pack aux parties reprises et 8 à 61 s sans). Absent des packs antérieurs et d'un site sans départ dans sa fiche |
| `valid_for` | `until` : la plus proche `valid_until` des couches, `until_layer`, note en français |
| `personal_sources` | si les sources à usage personnel ont été incluses |
| `basemaps[]` | `id`, titre, `tiles` (PMTiles), `style`, zooms, nombre de tuiles, taille, date des données, sources, notes |
| `layers[]` | `id`, titre, `kind` (`vector`, `raster`, `data`), `format` (`geojson`, `json`, `pmtiles`, `skr+gzip`, `tsv`), `file`, `meta`, `origin` (fichier du pipeline), `generated_at`, `valid_from`, `valid_until`, `validity`, `sources` (`common::meta::SourceRef` tels quels), `personal_use_only`, taille, nombre d'entités et types de géométrie, `visible`, `raster`, `classes` (couches de classes en vecteur), notes |
| `layers[].classes` | zooms, `extent`, nombre de tuiles, `source_layer`, `property` (`class`), légende (`value`, `label`, couleur de référence du thème clair), raster source, méthode, seuils des petites taches, tolérance et sommets par zoom, nombre de polygones, surfaces par classe (`classes` : raster, après lissage, polygones par zoom) et par seuil (`levels` : surface perdue, gagnée, polygones, zones significatives conservées) |
| `layers[].raster` | zooms, taille de tuile, nombre de tuiles, `encoding` (`palette_index` : l'indice de palette est la classe ; `rgba_legend` : tuiles tierces), `nodata_index`, légende (`value`, `label`, `color` `#rrggbbaa`), raster source, méthode de sous-échantillonnage |
| `voice` | emplacement des clips (VO3) : `status` (`present`, `absent`), `manifest`, `clips_dir`, nombre, taille, sources |
| `missing[]` | couches cherchées et absentes, avec la raison (absente du pipeline, source à usage personnel sans l'option, outil manquant) |
| `attributions[]` | attributions à afficher, sans doublon |
| `files[]`, `total_bytes` | chaque fichier (sauf le manifeste) avec taille et SHA-256 ; `Pack::verify` les recontrôle |

Toutes les URL des styles et tous les chemins du manifeste sont relatifs à la racine du pack (`pmtiles://basemap/osm.pmtiles`, `basemap/osm/fonts/{fontstack}/{range}.pbf`) ; le visualiseur (et plus tard l'app) les rend absolus. Le lecteur refuse un chemin qui sort du pack.

## Suite

- [Couches reprises, couches de classes en vecteur, pistes, zones, sources à usage personnel, rasters](format/couches.md)
- [Tailles des packs et contrôles](format/tailles.md)
