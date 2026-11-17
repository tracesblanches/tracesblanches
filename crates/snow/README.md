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
cargo run --release -p skimap-snow --example snow_build [AAAA-MM-JJ] [<site>]
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
| Jeu de test jurassien (39,8 km²) | **55 Haut-Doubs** : 93,2 % de la surface et le centre du village ; 56 Haut-Jura : 6,0 % (bord sud-ouest, à 2,2 km du centre) ; 0,8 % hors polygone |
| Jeu de test alpin (110,3 km²) | **17 Queyras** : 100 % |
| Arvieux, Mouthe | 17 Queyras 100 % ; 55 Haut-Doubs 99,7 % |

Les emprises de travail débordent : la carte prend donc le massif **par pixel**. Jeu de test alpin : Queyras 383 163 pixels, Thabor (13, coin nord-ouest) 21 871 pixels. Jeu de test jurassien : Haut-Doubs 153 140, Haut-Jura 159 776 (Risoux et Bellefontaine). Les pixels français hors de tout polygone prennent le massif du pixel le plus proche : 6 976 (1,7 %) et 12 714 (4,1 %).

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

Un oracle Python (hors dépôt) recalcule la carte à partir de `dem_5m.tif`, du CSV brut et des polygones (shapely) : écart maximal 0,00017 cm sur 400 906 pixels (jeu de test alpin) et 309 686 pixels (jeu de test jurassien) au 15/02/2026, aucune classe différente, aucune valeur d'une recherche exhaustive hors de la fourchette (300 pixels tirés), massif identique à shapely sur 3 000 pixels tirés par site. La carte du 13/11/2026 est cohérente avec le rapport 06 : Haut-Jura et Haut-Doubs à 0 cm partout, Queyras enneigé seulement en haute altitude (plat : 0 cm jusqu'à 2 400 m, 34 cm à 2 700 m ; carte du jeu de test alpin : moyenne 8 cm, maximum 84 cm).

### Taille (pack téléphone) et temps

Grille de 664 × 697 pixels (jeu de test alpin, 16,6 × 17,4 km) et 576 × 644 (jeu de test jurassien) ; 1,85 Mo et 1,48 Mo par raster Float32 non compressé.

| Fichier (Deflate) | Jeu de test alpin 15/02/2026 | Jeu de test jurassien 15/02/2026 | Jeu de test alpin 13/11/2026 |
|---|---|---|---|
| `snow_depth.tif` | 1 364 Ko | 1 044 Ko | 727 Ko |
| `snow_depth_low.tif` | 1 359 Ko | 809 Ko | 329 Ko |
| `snow_depth_high.tif` | 1 369 Ko | 1 087 Ko | 1 054 Ko |
| `class_groomed.tif` / `class_srn.tif` | 3 / 7 Ko | 18 / 13 Ko | 12 / 8 Ko |
| `massif.tif` | 3 Ko | 4 Ko | 3 Ko |
| Total par jour | ≈ 4,1 Mo | ≈ 3,0 Mo | ≈ 2,1 Mo |

Le Float32 se compresse mal (−26 %). Mesure pour le pack (P14) : les mêmes rasters arrondis au centimètre en UInt16 Deflate avec prédicteur font 209 / 198 / 235 Ko (jeu de test alpin) et 127 / 106 / 153 Ko (jeu de test jurassien).

Temps de la chaîne complète **par site et par jour** (`total` de `build`, 20 cœurs) :

| Cas | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Premier passage, cache vide (MNT à télécharger) | 210,5 s (MNT 207,7 s) | 83,5 s (MNT 83,2 s) |
| Premier passage du jour (nouveau fichier de simulation, 10,6 Mo) | 3,49 s (simulation 3,08 s) | 0,32 s (fichier déjà revalidé) |
| Passages suivants (revalidation 304) | 0,69 à 1,49 s | 0,61 à 0,69 s |
| Calcul seul, hors réseau (`SKIMAP_OFFLINE=1`) | 0,55 s | 0,25 s |

Hors réseau, l'écriture des GeoTIFF (0,25 s) puis la lecture du MNT en cache (0,14 s) et le décodage de la simulation (0,14 s) dominent ; la projection elle-même prend 0,014 s. En production, c'est le réseau qui domine : le téléchargement quotidien du fichier de simulation, et une fois pour toutes celui du MNT.

## Validation contre les stations (P7)

`skimap_snow::validate::validate(site, &layout, &downloader, 2020, 2025)` compare l'estimation aux hauteurs de neige mesurées à 06 h UTC, du 1er novembre au 31 mai des hivers 2020-2021 à 2025-2026, et écrit `data/out/<site>/snow/validation.json`.

```sh
cargo run --release -p skimap-snow --example snow_validate [2020 2025] [<site>]
```

- **Stations** (BDClim horaire, data.gouv, Licence Ouverte) : 05027001 (1 614 m, dans la commune) et Arvieux 05007003 (1 825 m) pour le jeu de test alpin ; Mouthe 25413001 (940 m) et La Boissaude Rochejean 25494001 (1 222 m), à quelques kilomètres au nord-est de l'emprise du jeu de test jurassien, dans le même massif (55 Haut-Doubs). Fichiers `H_<dép>_previous-2020-2024` et `H_<dép>_latest-2025-2026`.
- **Unité de `NEIGETOT` : le centimètre** (« hauteur de neige totale au sol (en cm) », `H_descriptif_champs.csv` officiel ; « cm, entier » dans le descriptif PDF du 03/06/2026). Heures en UTC pour la métropole (description du jeu sur data.gouv). Les valeurs de qualité 2 (« douteuse ») sont écartées.
- Deux estimations : **produit** = le pixel de 25 m qui contient la station, exactement comme sur la carte (calculé sur un carré de ± 500 m autour de la station, même grille) ; **plat** = la simulation sur le plat à l'altitude officielle de la station, qui isole la simulation et l'interpolation en altitude de la projection sur le relief.
- Biais = estimation − observation. Couverture = part des jours où l'observation tombe dans la fourchette [basse, haute].

### Résultats par site (stations regroupées)

| Site | Jours | Biais | MAE | RMSE | Moyenne observée | Plat : biais / MAE |
|---|---|---|---|---|---|---|
| Jeu de test alpin (05027001 + Arvieux) | 1 951 | −2,5 cm | 6,9 cm | 10,2 cm | 21,1 cm | −1,9 / 7,2 cm |
| Jeu de test jurassien (Mouthe + La Boissaude) | 2 304 | +4,1 cm | 6,7 cm | 13,8 cm | 6,8 cm | +3,7 / 6,3 cm |

### Par mois (produit ; biais / MAE en cm, moyenne observée entre parenthèses)

| Mois | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| novembre | −0,5 / 1,7 (3,1) | −0,5 / 0,8 (2,1) |
| décembre | −5,2 / 7,4 (22,2) | +0,2 / 5,5 (12,6) |
| janvier | −7,5 / 9,2 (38,0) | +2,5 / 8,5 (16,8) |
| février | −9,7 / 12,2 (47,6) | +12,8 / 14,4 (7,5) |
| mars | +0,9 / 12,0 (34,4) | +9,5 / 10,6 (3,1) |
| avril | +4,1 / 6,3 (3,9) | +2,2 / 2,7 (1,3) |
| mai | −0,1 / 0,2 (0,2) | +0,4 / 0,5 (0,1) |

### Par station

| Station | Pixel 25 m (alt., pente, orient.) | Période | Jours | Biais | MAE | RMSE | Plat : biais / MAE | Couverture (largeur moy.) | Neige ≥ 5 cm d'accord | Classes justes : damée / SRN |
|---|---|---|---|---|---|---|---|---|---|---|
| 05027001 (test alpin) | 1 616 m, 8,4°, 296° | 11/2020–05/2026 | 1 104 | −2,6 | 6,7 | 10,3 | −4,5 / 7,1 | 76 % (17,7 cm) | 91 % | 74 % / 85 % |
| Arvieux | 1 823 m, 7,6°, 165° | 11/2022–05/2026 | 847 | −2,4 | 7,2 | 10,1 | +1,5 / 7,3 | 61 % (18,6 cm) | 90 % | 78 % / 80 % |
| Mouthe | 943 m, 5,4°, 300° | 11/2020–04/2026 | 1 057 | +1,0 | 4,1 | 7,6 | +0,3 / 3,6 | 83 % (14,4 cm) | 84 % | 89 % / 95 % |
| La Boissaude Rochejean | 1 223 m, 1,7°, 285° | 11/2020–05/2026 | 1 247 | +6,8 | 8,9 | 17,4 | +6,5 / 8,6 | 70 % (21,3 cm) | 84 % | 78 % / 85 % |

Par saison (produit, biais / MAE en cm) : 05027001 −3,8 / 5,0 (2020-21), −1,8 / 3,5, +0,2 / 4,4, +7,4 / 8,4, −4,7 / 7,6, −10,1 / 10,2 (2025-26) ; Arvieux −4,1 / 8,7 (2022-23), +2,2 / 6,7, −8,1 / 8,5, +0,3 / 5,0 ; Mouthe +4,7 / 8,0, +2,7 / 8,1, −1,6 / 2,0, +0,1 / 0,5, −0,7 / 2,4, +0,4 / 3,1 ; La Boissaude +19,2 / 19,4, +20,6 / 21,0, −2,0 / 2,7, −1,7 / 2,2, +1,8 / 3,2, +3,1 / 4,7.

### Lecture

- **Alpes** : la simulation sous-estime le cœur de l'hiver d'environ 20 % (−5 à −10 cm de décembre à février pour 22 à 48 cm mesurés) et fond un peu tard (+4 cm en avril).
- **Jura** : bon de novembre à janvier (biais ≤ 2,5 cm), mais la neige simulée dure trop en février-mars (+10 à +13 cm alors que les stations de 940 à 1 222 m sont déjà déneigées). L'écart vient surtout de La Boissaude en 2020-21 et 2021-22 : la station a fondu début février 2021 (62 cm le 15/01, 0 cm le 23/02) alors que la simulation gardait de la neige ; les quatre autres saisons, |biais| ≤ 3,1 cm.
- Le produit et le calcul « plat » diffèrent peu : les stations sont sur des pentes faibles (1,7 à 8,4°). **La projection sur les pentes n'est donc pas validée par ces stations** ; il faut des relevés en pente (T5, #47).
- La fourchette (sensibilité au relief) ne contient que 61 à 83 % des observations : ce n'est pas un intervalle de confiance. Une marge d'erreur par mois tirée de ce tableau serait à calibrer sur d'autres stations que celles qui servent à la mesurer.
- Vérification indépendante : un oracle Python recalcule, depuis les fichiers bruts du cache, les jours retenus, le biais, la MAE et la RMSE des quatre stations, produit et plat : mêmes valeurs à 10⁻³ cm près. Les valeurs du pixel des stations 05027001 et d'Arvieux sont celles de la carte du site (altitude, pente et orientation recalculées depuis `dem_5m.tif` : mêmes valeurs à 10⁻³ près).
- Temps de la validation depuis le cache : 1,6 s (jeu de test alpin) et 1,5 s (jeu de test jurassien) pour six saisons ; le premier passage télécharge les fichiers horaires du département (51 Mo pour les Hautes-Alpes, 33 Mo pour le Doubs) et sept fichiers annuels de simulation (77 Mo, communs aux deux sites).
