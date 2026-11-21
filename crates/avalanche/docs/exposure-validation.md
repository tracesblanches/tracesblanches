[← skimap-avalanche](../README.md)

# Exposition aux avalanches : validation et limites

## Validation (P18) contre la CLPA (DDT 05), modèle seul

Jeu de test alpin, 173 emprises CLPA (171 de la commune + 2 qui touchent l'emprise du site), 27,9 km² :

| | Modèle seul | Après fusion CLPA (couche) |
|---|---|---|
| Surface CLPA « peu exposé » (objectif 0) | **1,68 %** (0,47 km²) | 0 |
| Rappel des classes exposées sur la CLPA | 98,3 % | 100 % |
| … dont « très exposé » | 70,4 % | 70,4 % |
| Surface CLPA atteinte par l'écoulement rare / fréquent | 94,9 % / 71,7 % | |
| Pistes nordiques dans une emprise : 5,778 km (rapport 07 : 5,78) ; en « peu exposé » | **1,41 km** | 0 |
| Classe 0 de la référence dans la CLPA | 0,43 % de la surface | |

**L'objectif n'est pas atteint par le modèle** : l'écoulement rare s'arrête avant le bas de 5 % de la surface des avalanches connues. Sensibilité (`exposure_sensitivity`) : c'est surtout le frottement en forêt (+5° dès qu'il y a un arbre, jusqu'à +52°).

| Variante (modèle seul) | CLPA « peu exposé » | Pistes CLPA « peu exposé » | Réseau nordique « peu exposé » |
|---|---|---|---|
| Défaut, marge 10 m | 1,68 % | 1,41 km | 5,88 km |
| Défaut, marge 25 m / 50 m | 0,79 % / 0,32 % | 1,02 / 0,47 km | 3,79 / 1,73 km |
| Écoulement rare sans forêt, marge 10 / 25 / 50 m | 0,30 / 0,12 / 0,03 % | 0,076 / 0,006 / 0,000 km | 0,89 / 0,27 / 0,04 km |
| α rare 15°, forêt, marge 10 / 25 / 50 m | 1,47 / 0,68 / 0,27 % | 1,27 / 0,87 / 0,30 km | 4,52 / 2,69 / 0,89 km |

Atteindre zéro sur la CLPA par le seul modèle ne laisse presque rien « peu exposé » sur le jeu de test alpin. D'où la fusion CLPA dans la couche ; elle ne protège pas des avalanches hors CLPA.

**EPA** (INRAE) : accessible sans clé par le WFS public `carto-service.inrae.fr/geoserver/siavalanches` (`epa_site_geom`, 376 objets dans l'emprise du jeu de test alpin le 15/11/2026 ; la carte `map.avalanches.fr` et les listes PDF par département) ; conditions du site : reproduction réservée à un « usage personnel excluant tout usage … d'information ». Exemple `epa_validation`, seulement avec `SKIMAP_PERSONAL_SOURCES=1`, rien n'est écrit dans la couche ; la géométrie `epa_site_geom` (axe ou contour du site) n'est pas documentée. Sur 244 sites avec données, 198 km de lignes : modèle seul 1,96 % de la longueur en « peu exposé » (68 sites touchés, 2 entièrement) ; couche après fusion CLPA 1,73 % (60 sites).

## Recalage du modèle seul (#31, #142)

Le pack public n'a pas la CLPA : le modèle seul doit couvrir les avalanches connues. Recalage sur les trois sites des Hautes-Alpes dont la référence existe en local (le jeu de test alpin et deux domaines nordiques du recensement) (CLPA DDT 05 et chemins EPA, données personnelles servant seulement de vérité de test, jamais dans un pack). Exemple `exposure_calibration` (rien n'est écrit) :

```sh
SKIMAP_PERSONAL_SOURCES=1 CALIB_MARGINS=10,15,20,25 cargo run --release -p skimap-avalanche --example exposure_calibration -- <site>...
```

Cause : le frottement de forêt d'AutoATES (+52° × couvert, seuil de vitesse 270, soit une hauteur de ligne d'énergie de 5 256 m jamais atteinte sous le plafond de 270 m) arrête l'écoulement rare dès les premiers arbres, quelle que soit sa vitesse ; et les manques se concentrent au bord des écoulements modélisés (sensibles à la marge).

Recalage retenu : écoulement rare avec seuils de vitesse de forêt à 50 (hauteur 180 m : au-delà, seul le frottement minimal de 5° s'applique ; une grande avalanche rare traverse la forêt), marge 20 m. Le scénario fréquent garde les valeurs d'AutoATES ; les comparaisons à la référence (`oracle_compare`, `oracle_connaught`) tournent avec `model_autoates`.

| Trois sites (CLPA 2,10 % au départ) | CLPA « peu exposé » | Pistes nordiques dans la CLPA « peu exposé » | EPA « peu exposé » (588 km) | Cellules « peu exposé » | Réseau nordique « peu exposé » (114 km) | Visites des écoulements |
|---|---|---|---|---|---|---|
| Avant : AutoATES, marge 10 m | 2,10 % | 1,41 km | 2,88 % | 9,7 % | 34,6 km | 1 |
| AutoATES, marge 25 m | 0,72 % | 1,02 km | 1,18 % | 5,1 % | 27,5 km | 1 |
| Frottement de forêt × 0,5, marge 20 m | 0,82 % | 1,03 km | 1,35 % | 5,8 % | 28,0 km | 1,05 |
| **Retenu : seuil 50, marge 20 m** | **0,64 %** | **0,16 km** | **0,77 %** | 5,5 % | 22,9 km | 1,07 |
| Seuil 30, marge 20 m | 0,46 % | 0,05 km | 0,53 % | 4,8 % | 19,3 km | 1,16 |
| Seuil 20, marge 20 m | 0,37 % | 0,04 km | 0,43 % | 4,4 % | 17,2 km | 1,19 |
| Sans frottement (détraînement gardé), marge 20 m | 0,39 % | 0,46 km | 0,72 % | 4,3 % | 20,8 km | 1,12 |

Par site, avant → après (CLPA ; EPA) : jeu de test alpin 1,68 % → 0,45 % ; 1,96 % → 0,60 % ; deuxième site 2,20 % → 0,78 % ; 2,37 % → 0,51 % ; troisième site 5,18 % → 1,56 % ; 4,55 % → 1,29 %. Sans forêt (troisième site seul : 0,16 %) coûte 61 % de visites de plus.

Temps de l'étape `avalanche-exposure` (seule étape changée), binaire de `main` contre le recalage, alternés deux fois, machine partagée (charge 50 à 90 sur 20 cœurs, d'où le temps CPU comme mesure) : jeu de test alpin 792 → 816 s CPU (+3,0 %), mur 112,2 → 115,0 s sur la paire calme ; deuxième site (le plus grand) 780 → 834 s CPU (+6,9 %). Mémoire (`model_memory`) : 107,6 → 106,5 o par cellule (limite 120).

Ce qui reste « peu exposé » dans la CLPA (0,64 %) est le prix de garder du terrain « peu exposé » ailleurs : la fusion CLPA reste dans la couche personnelle.

## Limites

- Licence : le portage suit de près le code de com4FlowPy (EUPL-1.2) et de Flow-Py (GPL-3.0) ; AutoATES v3.0 est MIT. L'effet sur la licence du projet (F2) est à décider.
- Forêt : le seuil de 3 m et la fenêtre de 15 m ne sont pas calibrés ; le frottement en forêt est le paramètre qui pèse le plus sur les manques CLPA.
- Les pentes hors de l'emprise du site (Italie, Suisse, sans LiDAR HD) ne produisent pas d'avalanche.
- Les paramètres sont ceux de l'ouest canadien (ISSW), sauf la forêt de l'écoulement rare et la marge, recalées sur trois sites des Hautes-Alpes seulement (aucune CLPA ni EPA locale ailleurs).

```sh
cargo run --release -p skimap-avalanche --example exposure_build -- <site>...
cargo run --release -p skimap-avalanche --example exposure_sensitivity -- <site>
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-avalanche --example epa_validation -- <site> [--product]
```
