[← skimap-avalanche](../README.md)

# Exposition aux avalanches : résultats

## Résultats (15/11/2026, machine de 20 cœurs, Intel Core Ultra 7 265K, 62 Go)

Accord Rust / référence, mêmes entrées (`oracle_comparison_<cell>m.json`) :

| | Jeu de test alpin 5 m | Jeu de test alpin 10 m | Jeu de test jurassien 5 m | Connaught 21 m (atelier) |
|---|---|---|---|---|
| Cellules classées | 10 087 708 | 2 518 015 | 7 789 943 | 42 011 |
| Pente, abri au vent | identiques au bit | identiques | identiques | identiques |
| Zones de départ (binaire, tamis) | identiques | identiques | identiques | identiques |
| Écoulement fréquent : zDelta, angle, nombre de chemins | identiques ; nombre de chemins 99,9988 % | identiques | identiques | identiques |
| Écoulement rare : zDelta / nombre de chemins identiques | 99,9997 % / 99,905 % | 99,998 % / 99,983 % | 100 % / 100 % | 100 % / 99,91 % |
| Classes ATES 0–4 finales | **100 %** (matrice diagonale) | 100,000 % (0 différence) | **100 %** | 99,981 % |
| Trois classes | **100 %** | 100 % | **100 %** | 99,991 % |

Les écarts résiduels de l'écoulement rare (±1 chemin sur 0,1 % des cellules atteintes à 5 m) ne sont pas expliqués : le kernel est porté opération par opération et l'écoulement fréquent est identique ; ils ne changent aucune classe sur nos deux sites. À Connaught, la grille du couvert de l'atelier est décalée d'environ 1 m de celle du MNT et la référence la rééchantillonne pour le classifieur ; nous non.

Temps, cas dominant (jeu de test alpin, emprise entière, 5 m, 11,5 M cellules, 6,15 M départs rares, 5,77 milliards de visites de cellules) :

| | Référence Python (numba) | Rust |
|---|---|---|
| Processus / fils | 10 | 10 / 20 |
| Écoulement rare | 402 s | 111 s / 63 s |
| Travail entier (de l'entrée aux classes) | **601 s** | **136 s / 79 s** (×4,4 / ×7,6) |
| Couche complète (`exposure_build`, MNH en cache, écritures, validation) | — | 82 à 91 s selon les passages (20 fils) ; 135 s au premier passage (téléchargement du MNH) |
| Jeu de test jurassien, travail entier | 49 s | 4,5 s |
| Mémoire maximale | 7,9 Go + 3,5 Go par processus | 1,8 Go (sensibilité, 20 fils) |

Couche utilisateur :

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Peu exposé / exposé / très exposé (cellules) | 9,3 / 51,8 / 38,8 % | 86,2 / 13,7 / 0,1 % |
| Réseau nordique de la commune, 34,5 km / 82,6 km | 12,5 % peu exposé | 86,2 % peu exposé |
| Rapport 07 (cône sans forêt) : part atteignable à 18° / 30° | 100 % / 69 % | 8,7 % / 1,0 % |
| Nous : part non « peu exposé » des longueurs avec donnée (boîte du rapport) | 87,5 % | 12,7 % (17,7 % en comptant les 6,8 km hors LiDAR HD) |

Sur le jeu de test jurassien, la part non « peu exposé » dépasse le majorant du rapport (8,7 %) : 59 % des cellules « exposé » sont en classe ATES 0–1 mais à moins de 10 m d'un petit talus raide (zone de départ avant tamis, souvent un bord de route ou de ruisseau), 41 % sont en classe ATES 2 (votes de pente sur 300 m et de clairières forestières du classifieur dev8).

## Accélération (X25, #159, 15/11/2026)

Résultats identiques au bit près : `exposure_fingerprint` (SHA-256 de chaque raster intermédiaire et final : pente, abri au vent, zones de départ, les quatre couches des deux écoulements, classes ATES, exposition) donne les mêmes empreintes avant et après sur les deux jeux de test (alpin et jurassien) ; les fichiers de la couche (`exposure_5m.tif`, `exposure_model_5m.tif`, `ates_5m.tif`, `canopy_5m.tif`, `segments.geojson`) et `summary.json` hors temps sont identiques octet pour octet. L'accord avec la référence AutoATES (100 % des classes) est donc inchangé.

- Écoulement : `tan β^exp` vers les 8 voisins ne dépend pas du chemin ; il est calculé une fois par cellule (par tuiles de 64 × 64, au premier passage) au lieu de deux `powf`, un `atan` et un `tan` par voisin ouvert et par visite ; l'angle de parcours est l'`atan` du maximum de Δh / d, pris une fois par cellule (`atan` monotone) ; parents lus dans la file au lieu d'être copiés ; état par fil mis à zéro paresseusement ; maxima atomiques sans écriture quand la valeur ne monte pas. Temps processeur d'un fil sur 194 M visites du jeu de test alpin : 47,6 s → 20,6 s (×2,3).
- Abri au vent : sélection des deux statistiques d'ordre du quantile au lieu du tri des ~450 angles, deux `atan` au lieu de 450.
- Écriture : emprises CLPA rastérisées une fois (au lieu de trois), validations, GeoTIFF et tronçons écrits en parallèle.

Mesures sur la machine partagée (charge 70 à 90 sur 20 cœurs, donc très dispersées) : modèle du jeu de test alpin 173 s → 118 s (extrême 131 → 97 s), jeu de test jurassien 11,4 → 4,3 s ; couche complète `exposure_build` jeu de test jurassien 23,5 → 6,8 s, jeu de test alpin 71 s après (159 à 186 s avant dans les mêmes conditions).
