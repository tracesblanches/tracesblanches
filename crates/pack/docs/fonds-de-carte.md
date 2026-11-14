# Fonds de carte par site (P4, #17)

[← skimap-pack](../README.md)

Deux fonds vectoriels par pack, chacun en PMTiles avec son style MapLibre, son sprite et ses glyphes, pour s'afficher sans réseau.

## Plan IGN v2 (`build::plan_ign`)

- Tuiles TMS `https://data.geopf.fr/tms/1.0.0/PLAN.IGN/{z}/{x}/{y}.pbf` de l'emprise du site, zooms 0 à 16, par le téléchargeur commun (source `ign`, 20 requêtes/s au plus, cache 30 jours). Les corps arrivent en protobuf brut ; l'archive les stocke en gzip. Un 404 isolé (vu une fois sur 16/33983/23609, 200 aux trois essais suivants) est réessayé quatre fois, comme le WMTS des pentes.
- Style `standard.json` de l'IGN réécrit : source `pmtiles://basemap/plan-ign.pmtiles`, sprite `PlanIgn` (1x et 2x) et glyphes des cinq piles « Source Sans Pro » copiés dans le pack, plages 0-255, 256-511 et 8192-8447 (latin, latin étendu A, ponctuation).
- Licence Etalab 2.0 ; stockage hors ligne ni interdit ni autorisé expressément par les CGU (L2, #7) : usage personnel.

## OpenStreetMap, Protomaps (`build::osm`)

- Extrait du build quotidien `https://build.protomaps.com/AAAAMMJJ.pmtiles` (139 Go, liste `build-metadata.protomaps.dev/builds.json` lue par le téléchargeur, source `protomaps`), emprise du site, zooms 0 à 15 (le maximum des builds).
- L'extrait se fait par requêtes partielles ; le téléchargeur ne met pas l'en-tête `Range` dans sa clé de cache, donc l'outil officiel `pmtiles extract` (go-pmtiles 1.31.2, BSD-3, binaire Linux x86_64 vérifié par SHA-256 `3ed7dbf4…a312f`, rangé dans `data/tools/`) fait l'extraction, chemin donné par `SKIMAP_PMTILES_BIN`. L'extrait est gardé dans `data/cache/protomaps/extracts/<site>-<build>-z<zoom>-<emprise>.pmtiles` et réutilisé.
- Style : `assets/protomaps/style-light-fr.json`, généré une fois par `assets/protomaps/generate.mjs` avec `@protomaps/basemaps` 5.7.2 (thème « light », libellés en français) ; polices Noto Sans (Regular, Medium, Italic) et sprite `v4/light` téléchargés depuis `protomaps.github.io/basemaps-assets` (source `protomaps-assets`). La pile Devanagari du style n'est pas copiée.
- Licence ODbL (Produced Work), attribution « Protomaps © OpenStreetMap ».

## Mesures (14-15/11/2026, build Protomaps 20261114)

Taille de l'archive selon le zoom maximal (`examples/basemap_sizes.rs` : mêmes tuiles réécrites en PMTiles pour chaque zoom maximal ; le zoom 17 n'a été téléchargé que pour Cervières) :

| Zoom max | Plan IGN Cervières | Plan IGN Chapelle | OSM Cervières | OSM Chapelle |
|---|---|---|---|---|
| 12 | 2,50 Mo (36 tuiles) | 2,13 Mo (31) | 2,24 Mo | 2,81 Mo |
| 13 | 4,08 Mo (72) | 3,78 Mo (67) | 2,57 Mo | 3,23 Mo |
| 14 | 5,83 Mo (182) | 5,76 Mo (177) | 2,96 Mo | 3,76 Mo |
| 15 | 8,13 Mo (581) | 8,07 Mo (537) | 3,59 Mo | 4,55 Mo |
| **16** | **11,34 Mo (2 061)** | **11,16 Mo (1 829)** | — | — |
| 17 | 16,69 Mo (7 833) | non mesuré | — | — |

Les zooms 0 à 12 pèsent déjà 2 à 2,5 Mo : peu de tuiles, mais chacune couvre une large région.

**Choix : Plan IGN z0-16, OSM z0-15.**

- Plan IGN : à z16 la précision des coordonnées est de 0,1 m (tuile de 430 m à 45° N, 4 096 unités) ; au-delà, MapLibre agrandit le z16. Capture comparée au zoom 17,5 sur le col d'Izoard (z16 agrandi contre vraies tuiles z17) : routes, ruisseaux et chemins identiques, seuls les libellés changent de place. Le z17 coûterait +5,4 Mo (+47 %) et 5 772 requêtes de plus par site (≈ 25 min au débit observé de 3 à 4 tuiles/s) pour des couches qui n'apparaissent qu'aux zooms 17-18 (bornes, parcelles, adresses).
- OSM : 3,6 et 4,6 Mo pour tous les zooms du build ; aucune raison de couper.
- Avec style, sprite et glyphes : Plan IGN 13,1 Mo (Cervières) et 13,0 Mo (Chapelle-des-Bois), OSM 4,6 et 5,6 Mo.

L'extrait de `pmtiles extract` fait 3,57 et 4,53 Mo (581 et 537 tuiles), autant que l'archive réécrite par la crate : même contenu.

Temps : à froid, le téléchargement des tuiles IGN domine (≈ 2 000 tuiles par site, 3 à 4 tuiles/s observées avec une requête à la fois, soit 8 à 10 min) ; l'extrait Protomaps prend 13 s ; depuis le cache, chaque fond s'écrit en 0,2 à 0,3 s.

## Contrôles

- `tests/real_packs.rs` (`basemaps_are_complete`) : archives MVT lisibles par le lecteur, tuile z14 présente au centre de la commune, style pointant vers l'archive du pack, chaque glyphe requis présent.
- Visualiseur (Chrome sans tête) : les deux fonds s'affichent ; requêtes vers `127.0.0.1` seulement, hors les deux scripts du CDN.
- `pmtiles verify` (go-pmtiles) : `osm.pmtiles` passe ; `plan-ign.pmtiles` et les rasters écrits par la crate `pmtiles` 0.28.1 sont refusés (« Leaf directories offset=0 must not be 0 ») : la crate laisse `leaf_offset` à 0 quand il n'y a pas de répertoire de feuilles. Lecture correcte par go-pmtiles (`show`, `tile`), le client JavaScript et notre lecteur ; correction à faire dans la crate.
