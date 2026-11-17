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

## Limites

- Licence : le portage suit de près le code de com4FlowPy (EUPL-1.2) et de Flow-Py (GPL-3.0) ; AutoATES v3.0 est MIT. L'effet sur la licence du projet (F2) est à décider.
- Forêt : le seuil de 3 m et la fenêtre de 15 m ne sont pas calibrés ; le frottement en forêt est le paramètre qui pèse le plus sur les manques CLPA.
- Les pentes hors de l'emprise du site (Italie, Suisse, sans LiDAR HD) ne produisent pas d'avalanche.
- Les paramètres sont ceux de l'ouest canadien (ISSW) ; aucun recalage sur la CLPA n'a été fait.

```sh
cargo run --release -p skimap-avalanche --example exposure_build -- <site>...
cargo run --release -p skimap-avalanche --example exposure_sensitivity -- <site>
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-avalanche --example epa_validation -- <site> [--product]
```
