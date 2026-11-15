# Données, configuration, sites et projections

[← Architecture](../architecture.md)

## Arborescence des données

```text
data/                      racine, $SKIMAP_DATA_DIR ou ./data (seul /data est ignoré par git)
  cache/<source>/          cache du téléchargeur
  out/<site>/<couche>/     produits du pipeline par site
  out/france/<couche>/     produits nationaux : domaines (P3), sites (registre), srn (G5),
                           stations (H1, H2), refresh (résumé du rafraîchissement), catalog.json
```

Chemins fournis par `common::layout::DataLayout` (`cache_dir`, `out_dir`, `layer_dir`, `france_layer_dir`) ; `layout::FRANCE` (`france`) n'est l'identifiant d'aucun site.

## Configuration

Variables d'environnement lues par `common::config::Config::from_env` :

| Variable | Rôle |
|---|---|
| `SKIMAP_DATA_DIR` | répertoire des données (défaut `./data`) |
| `METEOFRANCE_API_KEY` | clé du portail Météo-France (BERA, AROME, observations) |
| `DATATOURISME_API_KEY` | clé de l'API DATAtourisme |
| `SKITOUR_API_KEY` | clé de l'API Skitour |
| `SKIMAP_RATE_LIMIT_<SOURCE>` | quota d'une source, ex. `SKIMAP_RATE_LIMIT_DATATOURISME=1000/h` |
| `SKIMAP_PERSONAL_SOURCES` | `1` ou `true` : `Config::personal_sources`, sources à usage personnel (défaut : non) |
| `RUST_LOG` | niveau des journaux du binaire |

Les clés sont des `Secret` qui s'affichent `***` : elles n'apparaissent dans aucun journal, aucun message d'erreur, aucun fichier du cache.

`personal_sources` est désactivé par défaut parce que ces sources n'autorisent pas la redistribution : widget Nordic France et page de la mairie de Cervières (HTML sans licence), état ENJ (aucune licence affichée, L1), API gratuite d'Open-Meteo (usage non commercial). Sans l'option, `meteo` et `status-poll` refusent de tourner, le recensement (P3) ignore Nordic France et `stations` n'écrit ni titres ni prix (pages des émetteurs sans licence) ; avec, chaque source concernée porte `personal_use_only: true` dans les métadonnées.

## Métadonnées des couches

Chaque couche écrit `<couche>.meta.json` selon un seul schéma, `common::meta::LayerMeta`, écrit par `common::output::write_json` (JSON indenté, fin de ligne finale, écriture atomique `<fichier>.tmp` puis renommage) :

- toujours présents : `layer`, `site` (identifiant ou `france`), `tickets`, `generated_at`, `valid_from`, `valid_until`, `validity` (en mots), `crs`, `sources`, `files`, `notes` ; `parameters` et `stats` quand la couche en a ;
- `sources[]` (`SourceRef`) : `name`, `url`, `licence`, `attribution`, `personal_use_only` ; selon la source `producer`, `edition`, `note`, `fetched_at`, `last_modified`, `etag`, `sha256` ;
- `files[]` (`FileMeta`) : `name`, `description` ; selon le fichier `licence`, `count`, `properties` (nom → sens), et pour un raster `unit`, `format`, `crs`, `cell_size`, `size_cells`, `nodata`, `bytes`.

Changements par rapport aux schémas propres à chaque crate (phase B) : `path` → `name` et `valid_at` → `valid_from` (osm, refuges) ; `license` → `licence`, `data_date` → `edition` (zones, coverage) ; `valid_for` → `validity` (terrain, snow) ; `properties` de couche → `properties` du fichier GeoJSON (zones, coverage) ; `files` en objets au lieu de lignes de texte (meteo, status, avalanche) ; voice : `valid_until` (texte) → `validity`, `generated_at` à la seconde, ajout de `tickets`, `files` (`manifest.json`, `clips/<id>.opus`), `parameters` (voix, modèle, graine, débit Opus) et `stats` (clips par sorte, noms, octets, durée, temps de génération). Les fichiers de données sont inchangés octet pour octet, sauf : fin de ligne finale ajoutée aux JSON de meteo, status, avalanche et zones ; ordre des entités de `crossings.geojson` (P20 lit maintenant la couche `pistes`, mêmes 77 tronçons). Les GeoTIFF sont identiques pixel par pixel ; leurs octets compressés diffèrent selon le binaire (`skimap` lie le moteur zlib-rs de `zip` à flate2, l'exemple de la crate terrain miniz_oxide).

GeoTIFF : `common::geotiff` (fonction `geotiff`) : Float32 en lecture et écriture (terrain, snow), UInt8 en lecture (`read_u8` : classes d'exposition aux avalanches, lues par avalanche, assist, pack), lecture tolérante de tout type d'échantillon et de toute projection (`read_any` : sorties GDAL de l'oracle AutoATES). Les crates n'embarquent plus leur propre lecteur TIFF.

## Sites

Registre `common::site::Registry`, lu par toutes les sous-commandes depuis `data/out/france/sites/sites.json` (écrit par `skimap domaines`, G2 #118 ; sans lui, seuls les deux sites d'évaluation existent). Au 15/11/2026 : **272 sites**, dont 268 construits par `build-all` (les 4 autres sont découpés en tronçons) :

| Type (`kind`) | Sites | Origine |
|---|---|---|
| `nordic_station` | 219 | un par domaine du recensement P3 (`data/out/france/domaines/`) |
| `srn_area` | 42 : 40 domaines de jeu, 2 tronçons | zones SRN hors stations, `skimap srn` (G5 #131) |
| `traverse` | 11 : 3 traversées, 8 tronçons | traversées SRN (GTJ, Vercors) |

Chaque fiche (`SiteRecord`) : `id` (nom sans accents, `site::slug` ; chiffres du recensement pour les homonymes), `name`, `kind`, `census_id`, `department`, `insee`, `commune`, `bbox` (WGS84 : enveloppe + 2 km comptés en Lambert-93), `envelope_bbox`, `massifs` (code, nom, part), `massif_status` (`verified` 264, `outside` 6, `assumed` 2), `has_bera` (156), `status_source` (rapport 08), `km_nordic`, `sections` / `parent` (au-delà de 25 km de côté, tronçons recouvrants de 1 km). Règles détaillées : [france.md](france.md).

Données propres à un site dans des tables : `crates/common/data/sites.json` (les deux sites d'évaluation : emprise, commune et massifs vérifiés, sources d'état, départs, jour archivé, demandes de plan) et `crates/common/data/status_sources.json` (source d'état de chaque domaine) :

- Cervières (05), INSEE 05027, emprise 44.77,6.66,44.92,6.86 ; massif 17 Queyras (100 % de la commune) ; BERA ;
- Chapelle-des-Bois (25), INSEE 25121, emprise 46.54,6.02,46.68,6.20 ; massifs 55 Haut-Doubs (93,2 %) puis 56 Haut-Jura (6,0 %) ; pas de BERA dans le Jura.

Rattachement vérifié sur les polygones snowtools (`UMR-CNRM/snowtools`, `massifs.shp`) et le contour des communes. Les emprises débordent sur l'Italie et la Suisse : les crates qui ne veulent que la France découpent avec les contours des communes.

## Projections et grilles

- `common::crs` (proj4rs, pur Rust) : WGS84 (EPSG:4326), Lambert-93 (EPSG:2154), Web Mercator (EPSG:3857), emprise projetée densifiée, tuiles XYZ. Testé au millimètre contre PROJ 9.8.1 (pyproj 3.8.0) sur huit points dont les deux sites.
- `common::grid` : grille `Grid<T>` orientée nord, géotransformation au format GDAL, accès par cellule ou par coordonnées, interpolation bilinéaire avec valeur absente.
- `common::geom` : `LonLat`, `BBox` (ordre sud, ouest, nord, est explicite), distance haversine, conversions `geo-types`.
