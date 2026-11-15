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
layers/<id>.json                données (règles, BERA, synthèses, graphe de pistes, météo)
layers/offpiste-*.skr.gz        rasters du routeur hors piste de core (SKR1, gzip)
layers/meta/<couche>/<couche>.meta.json   copies des métadonnées des couches
rasters/<id>.pmtiles            rasters en tuiles PNG 256 px
voice/manifest.json, voice/clips/*.opus, voice/voice.meta.json
```

`manifest.json` :

| Champ | Contenu |
|---|---|
| `format`, `format_version` | `skimap-pack`, `1` ; le lecteur refuse un autre format ou une version plus récente |
| `pack_version` | heure de construction UTC `AAAAMMJJTHHMMSSZ`, croissante |
| `generator`, `generated_at`, `site` | auteur, date, site (identifiant, nom, département, emprise `[O, S, E, N]`, centre) |
| `valid_for` | `until` : la plus proche `valid_until` des couches, `until_layer`, note en français |
| `personal_sources` | si les sources à usage personnel ont été incluses |
| `basemaps[]` | `id`, titre, `tiles` (PMTiles), `style`, zooms, nombre de tuiles, taille, date des données, sources, notes |
| `layers[]` | `id`, titre, `kind` (`vector`, `raster`, `data`), `format` (`geojson`, `json`, `pmtiles`, `skr+gzip`, `tsv`), `file`, `meta`, `origin` (fichier du pipeline), `generated_at`, `valid_from`, `valid_until`, `validity`, `sources` (`common::meta::SourceRef` tels quels), `personal_use_only`, taille, nombre d'entités et types de géométrie, `visible`, `raster`, notes |
| `layers[].raster` | zooms, taille de tuile, nombre de tuiles, `encoding` (`palette_index` : l'indice de palette est la classe ; `rgba_legend` : tuiles tierces), `nodata_index`, légende (`value`, `label`, `color` `#rrggbbaa`), raster source, méthode de sous-échantillonnage |
| `voice` | emplacement des clips (VO3) : `status` (`present`, `absent`), `manifest`, `clips_dir`, nombre, taille, sources |
| `missing[]` | couches cherchées et absentes, avec la raison (absente du pipeline, source à usage personnel sans l'option, outil manquant) |
| `attributions[]` | attributions à afficher, sans doublon |
| `files[]`, `total_bytes` | chaque fichier (sauf le manifeste) avec taille et SHA-256 ; `Pack::verify` les recontrôle |

Toutes les URL des styles et tous les chemins du manifeste sont relatifs à la racine du pack (`pmtiles://basemap/osm.pmtiles`, `basemap/osm/fonts/{fontstack}/{range}.pbf`) ; le visualiseur (et plus tard l'app) les rend absolus. Le lecteur refuse un chemin qui sort du pack.

## Couches reprises

Lues dans `data/out/<site>/` avec leurs `<couche>.meta.json` (schéma `common::meta::LayerMeta` ; les métadonnées hors schéma — `voice`, `offpiste`, `routes` au 15/11/2026 — sont lues avec tolérance et une note le signale) :

| Couche du pack | Origine | Remarque |
|---|---|---|
| `pistes`, `pistes-graph-edges`, `pistes-graph-nodes` | `pistes/` | |
| `domaines` | `domaines/` | |
| `zones` (+ `zones-rules`) | `zones/` | propriétés ajoutées : `rules` (règles de `rules.json` qui s'appliquent à la zone, jointure `skimap_zones::Rule::applies_to_zone`) et `rules_text` (texte français) |
| `no-network`, `coverage-area` | `coverage/` | |
| `avalanche-areas`, `avalanche-crossings`, `avalanche-summary` | `avalanche-historic/` | vides à Chapelle-des-Bois (pas de CLPA) |
| `bera` | `bera/` | |
| `refuges-info`, `refuges-osm`, `refuges-camptocamp`, `refuges-links` | `refuges/` | trois fichiers, pas de fusion (licences, L3) |
| `status`, `status-summary` | `status/` | usage personnel : seulement avec `personal_sources` |
| `meteo-hourly` | `meteo-hourly/` | usage personnel |
| `routes`, `route-offpiste-*`, `piste-graph` | `routes/` | itinéraires de preuve (C1, C3, C5), graphe de pistes typé du noyau |
| `offpiste-elevation`, `-slope`, `-zones`, `-pistes`, `-exposure`, `-zone-classes`, `-snow` | `offpiste/` | rasters SKR1 de core, gzip ; neige : la date la plus récente ; `-exposure` : exposition aux avalanches, surcouche par défaut du routeur |
| `avalanche-segments` | `avalanche-exposure/segments.geojson` | tronçons des pistes par classe d'exposition (« terrain peu exposé (modèle) », « exposé », « très exposé ») ; 275 ko à Cervières, 810 ko à Chapelle-des-Bois (sommets des pistes OSM, plus un sommet interpolé par changement de classe) |
| `assist-briefing`, `assist-wax`, `assist-snow-quality`, `assist-suggestions` | `assist/` (`briefing_<date>.json`, `wax_`, `snow_quality_`, `suggestions_<date>.geojson`) | sorties de `skimap assist` de la dernière date ; usage personnel (elles lisent la météo Open-Meteo et l'état des pistes) : seulement avec `personal_sources` |
| `slope` | `terrain/slope_5m.tif` | tuiles z10-15, classes de la carte des pentes IGN |
| `avalanche-exposure` | `avalanche-exposure/exposure_5m.tif` (UInt8 1 / 2 / 3, 255 absente) | tuiles z10-15, palette comme la pente (vert pâle, orange, rouge) ; jamais « sûr » |
| `snow-depth`, `snow-groomed`, `snow-srn` | `snow/*.tif` | tuiles z9-13 |
| `ign-slopes` | WMTS `SLOPES.MOUNTAIN` (cache de terrain) | comparaison, tuiles RGBA z11-15 telles que servies |

## Rasters

Tout GeoTIFF Float32 du pipeline (`common::geotiff`, projection quelconque de `common::crs`) devient une couche par une entrée de `build::raster::catalog()` : classes (bornes, libellés, couleurs), zooms, sens des égalités. Au zoom maximal, chaque pixel prend la classe de la cellule source sous son centre (projection exacte pixel par pixel, rayon parallèle) ; les zooms inférieurs prennent la classe la plus fréquente des 2 × 2 pixels (égalité : la plus haute pour la pente, la plus basse pour la neige, le côté prudent). PNG 8 bits à palette : l'indice est la classe, la dernière entrée transparente est « pas de donnée ». `Pack::raster_value_at(id, position)` relit la classe au zoom maximal (décodeur PNG de la crate, sans dépendance). Les rasters de la neige passeront tels quels quand snow écrira des centimètres entiers : la classe se calcule sur la valeur.

## Tailles des packs (15/11/2026, sorties du pipeline de 00 h 52)

Packs construits avec `personal_sources` (usage personnel de l'auteur) et sans les tuiles IGN de comparaison :

| Partie | Cervières | Chapelle-des-Bois |
|---|---|---|
| Fond Plan IGN v2 (z0-16, style, sprite, glyphes) | 13,13 Mo | 12,95 Mo |
| Fond OSM Protomaps (z0-15, style, sprite, glyphes) | 4,60 Mo | 5,57 Mo |
| Couches `layers/` | 28,02 Mo | 21,09 Mo |
| dont rasters hors piste de core (gzip) : altitude / pente | 17,57 / 7,21 Mo | 10,64 / 4,97 Mo |
| Rasters en tuiles (pente z10-15, neige z9-13) | 3,07 Mo | 0,59 Mo |
| Voix (clips Opus) | 3,21 Mo | 4,21 Mo |
| **Total** | **52,06 Mo**, 788 fichiers | **44,42 Mo**, 1 028 fichiers |
| Couches à usage personnel incluses | 0,09 Mo | 0,55 Mo |
| Option tuiles IGN des pentes (z11-15, RGBA) | +22,63 Mo | +13,17 Mo |

L'altitude 5 m du routeur hors piste pesait un tiers du pack : core écrit maintenant ses rasters SKR1 en différences le long des lignes (octets faibles puis forts), et le gzip de l'altitude passe de 17,1 à 8,6 Mo à Cervières, de 10,5 à 5,7 Mo à Chapelle-des-Bois (zlib 6). Le format SKR1 appartient à core (`crates/core/docs/horspiste.md`).

Temps de construction depuis les caches : 1,0 à 1,1 s par site ; la pente (z10-15, 540 tuiles à Cervières) se calcule en 0,2 s et s'encode en 0,25 s.

## Contrôles

- `tests/synthetic_pack.rs` : pack construit sur des sorties synthétiques sans réseau, relu, exporté ; sources à usage personnel écartées sans l'option.
- `tests/real_packs.rs` (données réelles, `--ignored`) : `Pack::verify` sans écart ; chaque couche GeoJSON garde toutes les entités de sa source ; jointure zones-règles identique à la liste de la crate zones (`zones.meta.json` : 23 paires à Cervières, 33 à Chapelle-des-Bois) ; tous les pixels contrôlés (un sur neuf) des tuiles au zoom maximal égaux à la classe de la cellule GeoTIFF sous leur centre, 0 écart sur 3,4 M (pente Cervières), 3,0 M (pente Chapelle) et 0,27 M (chaque neige) ; part des pentes ≥ 30° lue dans les tuiles : 39,6 % et 3,7 %, comme la crate terrain (39,6 % et 3,7 %).
