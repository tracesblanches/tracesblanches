# skimap-pack

Pack hors ligne par site : fonds de carte, format de pack, lecteur côté téléphone, visualiseur, export pour Locus Map et OsmAnd. Tickets : P4 (#17), P14 (#27), P15 (#28), P21 (#100) ; mises à jour et synchronisation : K2 (#52), S0 (#50), E1 (#125).

## Deux usages

- **Sans fonctionnalité** : le lecteur (`reader::Pack`, `reader::TileArchive`, `manifest`, `png`, `tiles`) et la mise à jour (`update` : paquets, `apply_bundle` ; `sync` : index du serveur, `plan`, état). Aucune pile HTTP ni SQLite (`cargo tree -p skimap-pack -e normal` : flate2, geojson, hex, pmtiles, serde, serde_json, sha2, skimap-common, thiserror) ; compile pour `aarch64-linux-android` (`cargo ndk -t arm64-v8a --platform 34 build --release -p skimap-pack`).
- **Fonctionnalité `pipeline`** : constructeur du pack (`build`), fonds de carte (`build::plan_ign`, `build::osm`), rasters (`build::raster`), export Locus / OsmAnd (`export`), petit serveur HTTP pour le visualiseur (`serve`).

Ligne de commande : `skimap build-pack <site>`, `skimap export <site>`, `skimap serve [racine]` (et en fin de `skimap build-all <site>`).

```sh
# pack des deux sites (data/out/<site>/pack/)
cargo run --release -p skimap-pack --features pipeline --example build_pack
# export Locus Map / OsmAnd (data/out/<site>/export/)
cargo run --release -p skimap-pack --features pipeline --example export_pack
# visualiseur
cargo run --release -p skimap-pack --features pipeline --example serve -- . 127.0.0.1:8765
google-chrome "http://127.0.0.1:8765/tools/viewer/index.html?pack=../../data/out/cervieres/pack/"
# mesures P4 (warm : remplit le cache IGN par tranches de temps bornées)
cargo run --release -p skimap-pack --features pipeline --example basemap_sizes -- warm cervieres 17 150
cargo run --release -p skimap-pack --features pipeline --example basemap_sizes -- 17 cervieres
# contrôles
cargo test -p skimap-pack --features pipeline
cargo test --release -p skimap-pack --features pipeline --test real_packs -- --ignored --nocapture
```

Variables de `build_pack` : `SKIMAP_PERSONAL_SOURCES=1` (`Config::personal_sources` : état des pistes, météo heure par heure), `SKIMAP_PLAN_IGN_MAXZOOM`, `SKIMAP_PROTOMAPS_BUILD=AAAAMMJJ`, `SKIMAP_PACK_IGN_SLOPES=1` (tuiles IGN des pentes pour comparer, +13 à +23 Mo), `SKIMAP_PACK_SKIP=plan-ign,osm,voice` (développement).

API : `skimap_pack::build::build(site, &layout, &downloader, &BuildOptions) -> BuildReport` ; `skimap_pack::export::export(&Pack, dir) -> ExportReport` ; `skimap_pack::Pack::open(dir)`.

## Sommaire

1. [Fonds de carte (P4)](docs/fonds-de-carte.md) : Plan IGN v2 et OSM Protomaps, tailles par zoom, choix
2. [Format du pack (P14)](docs/format.md) : arborescence, manifeste, couches reprises, rasters, tailles
3. [Visualiseur (P15)](../../tools/viewer/README.md) : ouverture sur le poste et sur le Pixel
4. [Export Locus Map / OsmAnd (P21)](docs/export.md) : fichiers, validation, liste de vérification
5. [Mises à jour et synchronisation (K2, S0, E1)](docs/mise-a-jour.md) : index, paquets, plan de synchro, application côté téléphone, mesures

Le serveur `serve` (bibliothèque standard, une connexion par fil, `GET`/`HEAD`, `206 Partial Content`, rien hors de la racine) remplace `python3 -m http.server`, qui ignore l'en-tête `Range` : le client PMTiles refuse alors l'archive (« Server returned no content-length header or content-length exceeding request »).

## Limites

- Plan IGN v2 : stockage hors ligne ni interdit ni autorisé expressément par les CGU (L2, #7) ; carte des pentes IGN : usage hors ligne à confirmer (features.md n° 5). Usage personnel.
- `pmtiles` 0.28.1 laissait `leaf_offset = 0` sans répertoire de feuilles, refusé par `pmtiles verify` : copie corrigée dans `vendor/pmtiles` (`[patch.crates-io]`) ; les 14 archives des deux packs passent `pmtiles verify` (15/11/2026).
- Le visualiseur charge MapLibre et PMTiles depuis le CDN (cache du navigateur ensuite) ; les données ne viennent que du pack.
- Import sur le Pixel (Locus Map, OsmAnd) et essai du visualiseur sur le vrai téléphone : à faire (téléphone non branché le 15/11).
