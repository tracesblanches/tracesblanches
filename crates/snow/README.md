# skimap-snow

Carte d'enneigement estimée et sa validation contre les stations (pipeline). Tickets : P6 (#19), P7 (#20).

## Carte d'enneigement (P6)

`skimap_snow::build(site, &layout, &downloader, date)` écrit `data/out/<site>/snow/` pour une date (état à 06 h UTC) ou, sans date, pour la dernière date du fichier de l'année en cours (J-1) :

| Fichier | Contenu |
|---|---|
| `snow_depth.tif` | hauteur de neige naturelle estimée (cm), valeur centrale |
| `snow_depth_low.tif`, `snow_depth_high.tif` | fourchette (cm) |
| `class_groomed.tif` | classes piste damée : 0 insuffisant, 1 limite, 2 suffisant |
| `class_srn.tif` | classes ski de randonnée nordique (hors piste) |
| `massif.tif` | numéro du massif Météo-France utilisé par pixel |
| `snow.meta.json` | sources, licences, date de validité, fichiers, méthode et limites |

Tous les rasters sont en GeoTIFF Float32 Deflate, Lambert-93 (EPSG:2154), pixel de 25 m, NaN = pas de donnée.

```sh
cargo run --release -p skimap-snow --example snow_build [AAAA-MM-JJ] [cervieres|chapelle-des-bois]
SKIMAP_OFFLINE=1 cargo run --release -p skimap-snow --example snow_build   # cache seul
```

### Sources

- **Simulation nivologique Météo-France** (data.gouv, Licence Ouverte 2.0) : un CSV gzip par année, mis à jour chaque jour pour l'année en cours ; par massif, date, bande d'altitude de 300 m, 17 combinaisons (plat ; 8 orientations à 20° et à 40°), `neigetot` en cm. L'état est celui de 06 h UTC (l'écoulement est cumulé sur ]J-1 06 h, J 06 h]). Le fichier de l'année en cours est revalidé à chaque passage (requête conditionnelle).
- **Contours des massifs** : `snowtools/DATA/massifs.shp` du dépôt `UMR-CNRM/snowtools` (CNRM, Météo-France / CNRS, licence CeCILL-C), Lambert-93, figé au commit `c84a1609` (22/01/2023). C'est la seule source trouvée qui contient les massifs du Jura : le `Metadata_massif_DP_2024` du rapport 04 (copie d'un tiers) n'a que les massifs à BERA, et Météo-France ne publie que la liste des massifs (PDF) avec la simulation.
- **Relief** : MNT LiDAR HD 5 m de `skimap-terrain`, moyenne 5 × 5 à 25 m.

### Massifs des deux sites (confirmés)

Test sur les polygones snowtools et les contours des communes (geo.api.gouv.fr) :

| Commune | Massif |
|---|---|
| Chapelle-des-Bois (39,8 km²) | **55 Haut-Doubs** : 93,2 % de la surface et le centre du village ; 56 Haut-Jura : 6,0 % (bord sud-ouest, à 2,2 km du centre) ; 0,8 % hors polygone |
| Cervières (110,3 km²) | **17 Queyras** : 100 % |
| Arvieux, Mouthe | 17 Queyras 100 % ; 55 Haut-Doubs 99,7 % |

Les emprises de travail débordent : la carte prend donc le massif **par pixel**. Cervières : Queyras 383 163 pixels, Thabor (13, coin nord-ouest) 21 871 pixels. Chapelle-des-Bois : Haut-Doubs 153 140, Haut-Jura 159 776 (Risoux et Bellefontaine). Les pixels français hors de tout polygone prennent le massif du pixel le plus proche : 6 976 (1,7 %) et 12 714 (4,1 %).

### Méthode par pixel

1. Altitude : moyenne des 25 cellules de 5 m (au moins 13 valides) ; grille commune à toutes les emprises (bords à 25 k − 0,25 m en X, 25 k + 0,25 m en Y), si bien qu'un pixel calculé autour d'une station est identique au pixel de la carte du site.
2. Pente et orientation de Horn à 25 m : l'échelle des classes de la simulation (0, 20, 40°), pas le micro-relief de 5 m. Un pixel sans pente (bord, voisin sans donnée) est traité comme plat.
3. Hauteur : linéaire en altitude entre les deux bandes encadrantes (bornée aux bandes extrêmes), linéaire en orientation entre les deux octants voisins, linéaire en pente entre plat, 20° et 40° (au-delà de 40° : valeur à 40°).
4. **Fourchette** : minimum et maximum de la même fonction sur altitude ± 150 m, pente ± 10° et orientation ± 45°, calculés exactement (la fonction est multilinéaire entre les nœuds : on évalue les coins et les nœuds intérieurs). C'est la sensibilité de la simulation au relief local, **pas l'erreur du modèle** : la validation (P7) mesure combien d'observations y tombent.

### Classes par pratique (seuils proposés)

| Pratique | 0 insuffisant | 1 limite | 2 suffisant |
|---|---|---|---|
| Piste damée (classique, skating) | < 20 cm | 20 à 40 cm | ≥ 40 cm |
| SRN (hors piste) | < 30 cm | 30 à 60 cm | ≥ 60 cm |

Raisons : la dameuse tasse la neige naturelle à environ la moitié de sa hauteur et il lui faut une sous-couche continue sur prairie ou chemin ; sous 20 cm le damage est improbable, entre 20 et 40 cm la piste est fragile. Hors piste, la neige doit recouvrir pierres, buissons, souches et murets sur un terrain varié, d'où des seuils plus hauts. Ces seuils sont des **propositions** à valider sur le terrain (T5, #47) et auprès des gestionnaires (ENJ, pisteurs) ; la classe s'applique à la valeur centrale.

### Ce qui n'est pas modélisé

Le transport de neige par le vent (crêtes soufflées, corniches, accumulations sous le vent), l'interception et l'ombre de la forêt (le Risoux est une forêt), le tassement par le damage et la neige de culture. La simulation est une valeur de massif par bande d'altitude, pas une mesure locale. Tout cela est écrit dans `snow.meta.json`.

### Vérification indépendante

Un oracle Python (hors dépôt) recalcule la carte à partir de `dem_5m.tif`, du CSV brut et des polygones (shapely) : écart maximal 0,00017 cm sur 400 906 pixels (Cervières) et 309 686 pixels (Chapelle-des-Bois) au 15/02/2026, aucune classe différente, aucune valeur d'une recherche exhaustive hors de la fourchette (300 pixels tirés), massif identique à shapely sur 3 000 pixels tirés par site. La carte du 13/11/2026 est cohérente avec le rapport 06 : Haut-Jura et Haut-Doubs à 0 cm partout, Queyras enneigé seulement en haute altitude (plat : 0 cm jusqu'à 2 400 m, 34 cm à 2 700 m ; carte de Cervières : moyenne 8 cm, maximum 84 cm).

### Taille (pack téléphone) et temps

Grille de 664 × 697 pixels (Cervières, 16,6 × 17,4 km) et 576 × 644 (Chapelle-des-Bois) ; 1,85 Mo et 1,48 Mo par raster Float32 non compressé.

| Fichier (Deflate) | Cervières 15/02/2026 | Chapelle-des-Bois 15/02/2026 | Cervières 13/11/2026 |
|---|---|---|---|
| `snow_depth.tif` | 1 364 Ko | 1 044 Ko | 727 Ko |
| `snow_depth_low.tif` | 1 359 Ko | 809 Ko | 329 Ko |
| `snow_depth_high.tif` | 1 369 Ko | 1 087 Ko | 1 054 Ko |
| `class_groomed.tif` / `class_srn.tif` | 3 / 7 Ko | 18 / 13 Ko | 12 / 8 Ko |
| `massif.tif` | 3 Ko | 4 Ko | 3 Ko |
| Total par jour | ≈ 4,1 Mo | ≈ 3,0 Mo | ≈ 2,1 Mo |

Le Float32 se compresse mal (−26 %). Mesure pour le pack (P14) : les mêmes rasters arrondis au centimètre en UInt16 Deflate avec prédicteur font 209 / 198 / 235 Ko (Cervières) et 127 / 106 / 153 Ko (Chapelle-des-Bois).

Temps de la chaîne complète **par site et par jour** (`total` de `build`, 20 cœurs) :

| Cas | Cervières | Chapelle-des-Bois |
|---|---|---|
| Premier passage, cache vide (MNT à télécharger) | 210,5 s (MNT 207,7 s) | 83,5 s (MNT 83,2 s) |
| Premier passage du jour (nouveau fichier de simulation, 10,6 Mo) | 3,49 s (simulation 3,08 s) | 0,32 s (fichier déjà revalidé) |
| Passages suivants (revalidation 304) | 0,69 à 1,49 s | 0,61 à 0,69 s |
| Calcul seul, hors réseau (`SKIMAP_OFFLINE=1`) | 0,55 s | 0,25 s |

Hors réseau, l'écriture des GeoTIFF (0,25 s) puis la lecture du MNT en cache (0,14 s) et le décodage de la simulation (0,14 s) dominent ; la projection elle-même prend 0,014 s. En production, c'est le réseau qui domine : le téléchargement quotidien du fichier de simulation, et une fois pour toutes celui du MNT.
