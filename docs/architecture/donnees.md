# Données, configuration, sites et projections

[← Architecture](../architecture.md)

## Arborescence des données

```text
data/                      racine, $SKIMAP_DATA_DIR ou ./data (ignorée par git)
  cache/<source>/          cache du téléchargeur
  out/<site>/<couche>/     produits du pipeline par site
```

Chemins fournis par `common::layout::DataLayout` (`cache_dir`, `out_dir`, `layer_dir`).

## Configuration

Variables d'environnement lues par `common::config::Config::from_env` :

| Variable | Rôle |
|---|---|
| `SKIMAP_DATA_DIR` | répertoire des données (défaut `./data`) |
| `METEOFRANCE_API_KEY` | clé du portail Météo-France (BERA, AROME, observations) |
| `DATATOURISME_API_KEY` | clé de l'API DATAtourisme |
| `SKITOUR_API_KEY` | clé de l'API Skitour |
| `SKIMAP_RATE_LIMIT_<SOURCE>` | quota d'une source, ex. `SKIMAP_RATE_LIMIT_DATATOURISME=1000/h` |
| `RUST_LOG` | niveau des journaux du binaire |

Les clés sont des `Secret` qui s'affichent `***` : elles n'apparaissent dans aucun journal, aucun message d'erreur, aucun fichier du cache.

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
