# Données, configuration, sites et projections

[← Architecture](../architecture.md)

## Arborescence des données

```text
data/                      racine, $SKIMAP_DATA_DIR ou ./data (seul /data est ignoré par git)
  cache/<source>/          cache du téléchargeur
  out/<site>/<couche>/     produits du pipeline par site
  out/france/<couche>/     produits nationaux (recensement des domaines, P3)
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

`personal_sources` est désactivé par défaut parce que ces sources n'autorisent pas la redistribution : widget Nordic France et page de la mairie de Cervières (HTML sans licence), état ENJ (aucune licence affichée, L1), API gratuite d'Open-Meteo (usage non commercial). Sans l'option, `meteo` et `status-poll` refusent de tourner et le recensement (P3) ignore Nordic France ; avec, chaque source concernée porte `personal_use_only: true` dans les métadonnées.

## Métadonnées des couches

Chaque couche écrit `<couche>.meta.json` selon un seul schéma, `common::meta::LayerMeta`, écrit par `common::output::write_json` (JSON indenté, fin de ligne finale, écriture atomique `<fichier>.tmp` puis renommage) :

- toujours présents : `layer`, `site` (identifiant ou `france`), `tickets`, `generated_at`, `valid_from`, `valid_until`, `validity` (en mots), `crs`, `sources`, `files`, `notes` ; `parameters` et `stats` quand la couche en a ;
- `sources[]` (`SourceRef`) : `name`, `url`, `licence`, `attribution`, `personal_use_only` ; selon la source `producer`, `edition`, `note`, `fetched_at`, `last_modified`, `etag`, `sha256` ;
- `files[]` (`FileMeta`) : `name`, `description` ; selon le fichier `licence`, `count`, `properties` (nom → sens), et pour un raster `unit`, `format`, `crs`, `cell_size`, `size_cells`, `nodata`, `bytes`.

Changements par rapport aux schémas propres à chaque crate (phase B) : `path` → `name` et `valid_at` → `valid_from` (osm, refuges) ; `license` → `licence`, `data_date` → `edition` (zones, coverage) ; `valid_for` → `validity` (terrain, snow) ; `properties` de couche → `properties` du fichier GeoJSON (zones, coverage) ; `files` en objets au lieu de lignes de texte (meteo, status, avalanche) ; voice : `valid_until` (texte) → `validity`, `generated_at` à la seconde, ajout de `tickets`, `files` (`manifest.json`, `clips/<id>.opus`), `parameters` (voix, modèle, graine, débit Opus) et `stats` (clips par sorte, noms, octets, durée, temps de génération). Les fichiers de données sont inchangés octet pour octet, sauf : fin de ligne finale ajoutée aux JSON de meteo, status, avalanche et zones ; ordre des entités de `crossings.geojson` (P20 lit maintenant la couche `pistes`, mêmes 77 tronçons). Les GeoTIFF sont identiques pixel par pixel ; leurs octets compressés diffèrent selon le binaire (`skimap` lie le moteur zlib-rs de `zip` à flate2, l'exemple de la crate terrain miniz_oxide).

GeoTIFF : `common::geotiff` (fonction `geotiff`) : Float32 en lecture et écriture (terrain, snow), UInt8 en lecture (`read_u8` : classes d'exposition aux avalanches, lues par avalanche, assist, pack), lecture tolérante de tout type d'échantillon et de toute projection (`read_any` : sorties GDAL de l'oracle AutoATES). Les crates n'embarquent plus leur propre lecteur TIFF.

## Sites

Définis dans `common::site` :

- Cervières (05), INSEE 05027, emprise 44.77,6.66,44.92,6.86 ; massif Météo-France 17 Queyras (100 % de la commune) ; BERA disponible.
- Chapelle-des-Bois (25), INSEE 25121, emprise 46.54,6.02,46.68,6.20 ; massifs 55 Haut-Doubs (93,2 % de la commune et le village) puis 56 Haut-Jura (côté Risoux, 6,0 %) ; pas de BERA dans le Jura.

Rattachement vérifié par P6 (#19) sur les polygones snowtools (`UMR-CNRM/snowtools`, `massifs.shp`) et le contour de la commune : `MassifStatus::Verified` pour les deux sites.

Les emprises débordent sur l'Italie et la Suisse : les crates qui ne veulent que la France découpent avec les contours des communes.

## Projections et grilles

- `common::crs` (proj4rs, pur Rust) : WGS84 (EPSG:4326), Lambert-93 (EPSG:2154), Web Mercator (EPSG:3857), emprise projetée densifiée, tuiles XYZ. Testé au millimètre contre PROJ 9.8.1 (pyproj 3.8.0) sur huit points dont les deux sites.
- `common::grid` : grille `Grid<T>` orientée nord, géotransformation au format GDAL, accès par cellule ou par coordonnées, interpolation bilinéaire avec valeur absente.
- `common::geom` : `LonLat`, `BBox` (ordre sud, ouest, nord, est explicite), distance haversine, conversions `geo-types`.
