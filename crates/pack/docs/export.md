# Export pour Locus Map et OsmAnd (P21, #100)

[← skimap-pack](../README.md)

`skimap_pack::export::export(&Pack, dir)` lit un pack par le lecteur (l'export montre exactement ce que le pack contient) et écrit `data/out/<site>/export/` :

| Dossier | Contenu |
|---|---|
| `kml/` | une couche par fichier, styles partagés (couleur de ligne, remplissage), un dossier par catégorie, description HTML : `pistes.kml` (dossiers par couleur), `refuges-*.kml` (une source par fichier, licences séparées), `zones-protegees.kml` (règles en description), `sans-reseau.kml`, `avalanches-troncons.kml`, `avalanches-emprises.kml`, `avalanches-exposition.kml` (pistes par classe d'exposition, classe dans le nom et la description, un dossier par classe), `boucles-suggerees.kml` (boucles de l'assistant, nom « 13 km, ~2 h, retour 12 h 15 », raisons en description, un dossier par demande ; avec `personal_sources`), `itineraires.kml` |
| `gpx/` | les mêmes couches en GPX 1.1, plus les pistes en un fichier par couleur (`pistes-vertes.gpx`…) : OsmAnd colore un fichier entier plus sûrement qu'une trace ; couleurs `osmand:color` / `osmand:width` (OsmAnd) et `gpx_style:line` (GPX Style 0.2, Locus) ; polygones en traces fermées (un segment par anneau) |
| `locus/*.mbtiles` | surcouches raster MBTiles 1.3 (`type=overlay`, lignes TMS) |
| `osmand/*.sqlitedb` | mêmes tuiles au format SQLiteDB d'OsmAnd : `tiles(x, y, z, s, image)`, table `info` avec `tilenumbering='simple'` (zoom non inversé, lu dans `SQLiteTileSource.java`), sans URL (hors ligne), `timecolumn='no'`, `tilesize=256` |
| `LISEZMOI.md`, `aide/*.md` | mode d'emploi en français : contenu et tailles, copie sur le téléphone, import Locus, import OsmAnd, liste de vérification, sources |

`assistant/assistant.txt` : textes du briefing, du fartage et de la qualité de neige du jour (avec `personal_sources`).

Surcouches : pentes (classes de la carte des pentes IGN, moins de 30° transparent), neige (hauteur, classes piste damée et hors piste) et exposition aux avalanches (`exposition-avalanches`, « terrain peu exposé (modèle) », « exposé », « très exposé » ; jamais « sûr »). OsmAnd n'affiche une surcouche que jusqu'à deux zooms au-dessus de son zoom maximal (`MapTileLayer.OVERZOOM_IN = 2`) : l'export ajoute un ou deux zooms agrandis (plus proche voisin, exact) au-dessus du zoom calculé du pack, jusqu'au zoom 16 pour la pente (affichée jusqu'au 18) et 15 pour la neige. Les tuiles IGN de comparaison ne sont pas exportées (usage hors ligne à confirmer). Abribus OSM écartés.

```sh
cargo run --release -p skimap-pack --features pipeline --example export_pack
```

## Validation (15/11/2026)

- **XML** : les 26 fichiers de Cervières et les 22 de Chapelle-des-Bois (15/11/2026, exposition aux avalanches et boucles suggérées comprises) passent `xmllint --noout --schema` contre `gpx.xsd` (GPX 1.1, topografix.com) et `ogckml22.xsd` (OGC KML 2.2, avec `atom-author-link.xsd` et `xAL.xsd` en local).
- **SQLite** : `PRAGMA integrity_check` = `ok` ; `pentes.mbtiles` de Cervières : zooms 10 à 16, 4 / 6 / 15 / 33 / 106 / 376 / 1 504 tuiles, `tile_row = 2^z − 1 − y` ; `pentes.sqlitedb` : mêmes colonnes et lignes XYZ ; une tuile tirée de la base est un « PNG image data, 256 x 256, 8-bit colormap ».
- **Tests** : `export::tests` (GPX, KML, agrandissement des tuiles), `db::tests` (deux formats), `tests/synthetic_pack.rs` (pack synthétique → export, tuiles OsmAnd zooms 10 à 16, échappement des descriptions).
- gpsbabel n'est pas installé sur le poste : non utilisé.

## Tailles

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Total (15/11/2026 ; 14/11 : 21,1 et 8,6 Mo) | 31,7 Mo | 32,4 Mo |
| Pentes (MBTiles / SQLiteDB) | 6,8 / 6,8 Mo, 2 044 tuiles | 1,3 / 1,3 Mo, 1 746 tuiles |
| Exposition aux avalanches (MBTiles / SQLiteDB) | 3,3 / 3,3 Mo, 2 044 tuiles | 2,0 / 2,0 Mo, 1 746 tuiles |
| Neige, trois surcouches (chaque format) | 1,2 Mo | 0,8 Mo |
| GPX + KML (24 et 20 fichiers) | 9,0 Mo (14/11 : 5,0) | 23,9 Mo (14/11 : 4,2), dont 20,3 Mo de `avalanches-exposition` (géométrie échantillonnée au mètre par la crate avalanche) |

Temps de l'export complet : 3,0 s (Cervières) et 1,4 s (Chapelle-des-Bois).

## Reste à faire

L'import sur le Pixel (Locus Map et OsmAnd) se fait avec la liste de vérification de `aide/verification.md` ; les chemins de menus du mode d'emploi sont à confirmer à cette occasion.
