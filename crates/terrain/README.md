# skimap-terrain

Relief IGN par zone, pente et orientation (pipeline). Ticket : P5 (#18).

## Ce que fait la crate

- `dem` : télécharge le MNT LiDAR HD de l'IGN rééchantillonné à 5 m par le WMS-R de la Géoplateforme (`IGNF_LIDAR-HD_MNT_ELEVATION.ELEVATIONGRIDCOVERAGE.LAMB93`, sans clé, Licence Ouverte), dalle de 1 km par dalle, par le téléchargeur commun (source `ign`, cache d'un an : le WMS n'envoie ni `ETag` ni `Last-Modified`). Mosaïque, découpe, moyenne par blocs.
- `slope` : pente et orientation par l'opérateur de Horn (celui de `gdaldem`).
- `ign_slopes` : comparaison avec la carte des pentes IGN `GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN` (WMTS, sans clé).
- `geotiff`, `png`, `meta` : lecture et écriture GeoTIFF Float32, décodeur PNG minimal (aucune crate PNG dans le workspace), `<couche>.meta.json`.

`skimap_terrain::build(site, &layout, &downloader)` écrit `data/out/<site>/terrain/` :

| Fichier | Contenu |
|---|---|
| `dem_5m.tif` | altitude (m, IGN69), GeoTIFF Float32 Deflate, Lambert-93 (EPSG:2154), 5 m, NaN = pas de donnée |
| `slope_5m.tif` | pente (degrés) |
| `aspect_5m.tif` | orientation : direction vers laquelle la pente descend, degrés depuis le nord dans le sens horaire, -1 = plat |
| `slope_ign_comparison.json` | comparaison avec la carte des pentes IGN |
| `terrain.meta.json` | sources, licences, dates, fichiers, notes |

```sh
cargo run --release -p skimap-terrain --example terrain_build [cervieres|chapelle-des-bois]
cargo run --release -p skimap-terrain --example research_tiles_check -- <research/raw/census_ava/ign/dem5>
```

## Détails de méthode

- Les dalles sont demandées exactement comme dans le rapport 07 (`BBOX` décalée de 0,25 m, `WIDTH=200`) : les 143 dalles de `research/raw/census_ava/ign/dem5/` sont identiques octet pour octet à celles du pipeline (exemple `research_tiles_check` : « 143 identical, 0 different »). La grille 5 m garde ce décalage de 0,25 m.
- Le WMS décrit la projection comme une conique conforme « définie par l'utilisateur » avec les paramètres de Lambert-93 et la citation `EPSG:2154` ; le lecteur l'accepte seulement si les six paramètres et la citation correspondent.
- Le WMS rééchantillonne les dalles de 50 cm : écart moyen de 0,11 m à la moyenne par blocs de 10 × 10 sur la dalle 0998-6426 (0,82 m au plus proche voisin).
- Au bord de la couverture LiDAR HD (Italie, Suisse), le WMS mélange la valeur de remplissage -9999 aux cellules voisines : jusqu'à 2 650 m d'écart à Cervières (317 m au lieu de 2 252 m). Les deux anneaux de cellules voisines du vide sont mis à NaN (`erode_valid`) ; après correction, l'altitude de Cervières va de 1 266 à 3 317 m (au lieu de -615 m avant).
- Le WMTS a répondu une fois 404 pour une dalle, puis 200 aux cinq requêtes suivantes : un 404 est réessayé avant d'échouer.

## Résultats (14/11/2026)

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Dalles LiDAR HD | 324 | 272 |
| Grille 5 m | 3 317 × 3 480 | 2 877 × 3 217 |
| Cellules avec altitude | 87,5 % | 84,3 % |
| Altitude | 1 266 à 3 317 m | 721 à 1 385 m |
| Pente ≥ 30° (toute l'emprise) | 39,6 % | 3,7 % |
| Taille dem / pente / orientation | 34 / 36 / 36 Mo | 25 / 28 / 28 Mo |

**Comparaison avec la carte des pentes IGN** (zoom 15, pixel ≈ 3,4 m, chaque pixel IGN classé par sa couleur de légende et comparé à la classe de notre pente dans la cellule de 5 m qui le contient) :

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Tuiles WMTS / pixels comparés | 399 / 19,9 M | 360 / 16,6 M |
| Même classe (< 30, 30-35, 35-40, 40-45, ≥ 45°) | 76,9 % | 97,8 % |
| Au plus une classe d'écart | 95,2 % | 99,5 % |
| Part ≥ 30° : IGN / nous | 33,0 % / 38,7 % | 3,25 % / 2,97 % |
| ≥ 30° : précision / rappel / F1 / IoU | 0,785 / 0,921 / 0,847 / 0,735 | 0,850 / 0,775 / 0,811 / 0,682 |
| Notre pente moyenne dans les classes IGN 30-35 / 35-40 / 40-45 / ≥ 45° | 33,4 / 37,9 / 43,2 / 51,9° | 31,1 / 34,1 / 38,6 / 47,6° |

Lecture : à Cervières notre pente est un peu plus forte que celle de l'IGN (≈ +1° au milieu des classes, 5,7 points de plus au-dessus de 30°) ; dans le Jura elle est un peu plus faible sur les ressauts raides. Les deux cartes ne partent pas du même MNT : RGE ALTI 5 m pour l'IGN, LiDAR HD rééchantillonné pour nous. 2 % des pixels IGN (bords anti-crénelés) n'ont pas une couleur de légende et ne sont pas comptés.

**Contrôle croisé avec le rapport 07** (même mosaïque de dalles) : la méthode du rapport (`numpy.gradient`) refaite sur ses dalles redonne 38,17 % et 2,35 % (publiés : 38,1 % et 2,3 %). L'altitude du pipeline est identique à celle des dalles du rapport (écart maximal 0 m sur 3,1 M et 2,6 M cellules). Notre pente de Horn en Rust est égale à une implémentation numpy indépendante à 4·10⁻⁶ ° près (orientation : 1,5·10⁻⁵ °). Avec Horn sur l'emprise complète de la mosaïque, la part ≥ 30° vaut 39,0 % et 3,0 % : l'écart au rapport vient de l'opérateur et du rapport qui compte les cellules sans pente comme non raides.

**Temps** : téléchargement initial du MNT 166 s (Cervières) et 149 s (Chapelle), puis 0,1 s depuis le cache ; écriture des trois GeoTIFF 5 m 3,4 à 4,2 s ; comparaison IGN 71 s la première fois, 0,1 à 0,9 s ensuite.

## Limites

- Pas de donnée hors de France ni dans les zones ZICAD masquées.
- La carte des pentes IGN ne sert qu'à la comparaison ; sa licence pour un usage hors ligne n'est pas confirmée (features.md n° 5).
