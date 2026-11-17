# skimap-pack

Pack hors ligne par site : fonds de carte, format de pack, lecteur côté téléphone, visualiseur, export pour Locus Map et OsmAnd. Tickets : P4 (#17), P14 (#27), P15 (#28), P21 (#100) ; mises à jour et synchronisation : K2 (#52), S0 (#50), E1 (#125).

## Deux usages

- **Sans fonctionnalité** : le lecteur (`reader::Pack`, `reader::TileArchive`, `manifest`, `png`, `tiles`) et la mise à jour (`update` : paquets, `apply_bundle` ; `sync` : index du serveur, `plan`, état). Aucune pile HTTP ni SQLite (`cargo tree -p skimap-pack -e normal` : flate2, geojson, hex, pmtiles, serde, serde_json, sha2, skimap-common, thiserror) ; compile pour `aarch64-linux-android` (`cargo ndk -t arm64-v8a --platform 34 build --release -p skimap-pack`).
- **Fonctionnalité `pipeline`** : constructeur du pack (`build`), fonds de carte (`build::plan_ign`, `build::osm`), rasters (`build::raster`), pente et exposition en polygones vectoriels (`build::class_tiles`), pistes enrichies de l'état et du plan des pistes (`build::pistes`), export Locus / OsmAnd (`export`), petit serveur HTTP pour le visualiseur (`serve`).

Ligne de commande : `skimap build-pack <site>`, `skimap export <site>`, `skimap serve [racine]` (et en fin de `skimap build-all <site>`).

```sh
# pack des deux sites (data/out/<site>/pack/)
cargo run --release -p skimap-pack --features pipeline --example build_pack
# export Locus Map / OsmAnd (data/out/<site>/export/)
cargo run --release -p skimap-pack --features pipeline --example export_pack
# visualiseur
cargo run --release -p skimap-pack --features pipeline --example serve -- . 127.0.0.1:8765
google-chrome "http://127.0.0.1:8765/tools/viewer/index.html?pack=../../data/out/<site>/pack/"
# mesures P4 (warm : remplit le cache IGN par tranches de temps bornées)
cargo run --release -p skimap-pack --features pipeline --example basemap_sizes -- warm <site> 17 150
cargo run --release -p skimap-pack --features pipeline --example basemap_sizes -- 17 <site>
# polygones de pente et d'exposition : mesures pour un seuil de petites taches
cargo run --release -p skimap-pack --features pipeline --example class_tiles -- patch=200 inner=400
# contrôles
cargo test -p skimap-pack --features pipeline
cargo test --release -p skimap-pack --features pipeline --test real_packs -- --ignored --nocapture
```

Variables de `build_pack` : `SKIMAP_PERSONAL_SOURCES=1` (`Config::personal_sources` : état des pistes, météo heure par heure, plan des pistes, zones Biodiv'sports, CLPA ; sans l'option, aucune de ces sources ni ce qui en dérive n'entre dans le pack), `SKIMAP_PLAN_IGN_MAXZOOM`, `SKIMAP_PROTOMAPS_BUILD=AAAAMMJJ`, `SKIMAP_PACK_IGN_SLOPES=1` (tuiles IGN des pentes pour comparer, +13 à +23 Mo), `SKIMAP_PACK_SKIP=plan-ign,osm,voice` (développement).

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

## Parties reprises du pack précédent (#164)

Un pack est refait dès qu'une de ses couches change, et chaque run AROME (quatre par jour au rattrapage) change la météo de chaque site. Les parties lourdes, qui ne dépendent pas de la météo, ne sont pas refaites quand leurs entrées n'ont pas changé (`build::parts`) : chacune a une clé, SHA-256 de tout ce dont dépendent ses octets, jamais une date :

| Partie | Clé |
|---|---|
| `class:slope`, `class:avalanche-exposure`, `class:snow-*` (polygones de classes) | taille et SHA-256 du GeoTIFF source et de sa métadonnée, description de la couche, paramètres de vectorisation, `personal_sources` |
| `basemap:osm` | dernier build Protomaps de la liste, build en usage (`extracts/<site>-z<zoom>-<emprise>.current.json`, X26) et présence de son extrait, emprise, zoom, style |
| `basemap:plan-ign` | emprise, zooms, style IGN servi, date de révision de la fiche IGN (qui décide des tuiles gardées en cache, X26) |

Toutes les clés portent la version de la crate et `parts::PARTS_REVISION`, à incrémenter quand le code fait d'autres octets avec les mêmes entrées. Une partie dont la clé est celle du pack en place reprend ses fichiers (lien physique, copie sinon) et son entrée du manifeste : mêmes octets, rien à télécharger pour le téléphone, et sa date reste celle de ses données. Les clés du pack en place sont dans `data/out/<site>/pack.parts.json`, à côté du pack et hors de lui (le téléphone ne le voit pas), écrit après la substitution ; une partie n'est reprise que si chacun de ses fichiers dans le pack en place a encore la taille et le SHA-256 notés avec la clé (relus). Le format du pack ne change pas. `BuildReport::reused` liste les parties reprises (ligne `build-pack` de `build-all`).

L'export Locus Map / OsmAnd fait de même pour ses surcouches (`locus/*.mbtiles`, `osmand/*.sqlitedb`, la partie lente) : clé = entrée de la couche dans le manifeste, taille et SHA-256 de son fichier, nom du site, `export::OVERLAY_REVISION` ; clés dans `data/out/<site>/export.parts.json`. La description d'une surcouche donne la date de ses données (`données du …`) au lieu de la version du pack.

Sans entrée changée, un nouveau build Protomaps (un par jour) refait seulement `basemap:osm` : téléchargement de l'extrait du nouveau build pour le comparer au build en usage (X26), une fois par site et par build.

## Fond de France (X20, #154)

`build::france` extrait du build Protomaps quotidien l'emprise de la France métropolitaine et de la Corse (`france::BBOX`), zooms 0 à `france::MAXZOOM` (6), par requêtes partielles, et y joint les glyphes des noms (Source Sans Pro, celles du Plan IGN) ; `france.json` décrit les fichiers (taille, SHA-256), le build et la date des données. Le build en usage est gardé tant que le dernier a moins de 7 jours et le même schéma : le téléphone ne retélécharge le fond qu'une fois par semaine au plus, et la date affichée reste celle du build en usage.

```sh
cargo run --release -p skimap-pack --features pipeline --example france_basemap -- data/out/france-basemap        # le fond
cargo run --release -p skimap-pack --features pipeline --example france_basemap -- /tmp/fr 5 6 7 8                # tailles par zoom
skimap-server publish-france data/out/france-basemap
```

Mesuré le 17/11/2026 (build 20261117, 22 tuiles au zoom 6) : zooms 0 à 5 : 1,00 Mo (1,32 Mo avec glyphes) ; 0 à 6 : 1,78 Mo (2,10 Mo) ; 0 à 7 : 8,56 Mo (8,88 Mo) ; 0 à 8 : 27,95 Mo (28,27 Mo). Retenu : 0 à 6, 2,10 Mo en 7 fichiers ; extraction 3,7 s depuis le réseau.
