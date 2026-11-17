# skimap-core, C1 : graphe et routage

[← skimap-core](../README.md)

- Sens unique : une arête `oneway` ne se parcourt que de `from` vers `to` ; le rattachement HMM (C2) l'ignore (un skieur peut se tromper de sens).
- Coût : somme sur les morceaux de 10 m du profil d'un coût (distance horizontale, dénivelé signé, altitude), multipliée par un facteur par arête. `Distance` donne le plus court chemin ; `EtaModel` (C5) donne le temps. Une arête sans profil (hors LiDAR HD : Suisse, Italie) est plate.
- Style (`StylePolicy`, réglable, `f64::INFINITY` interdit) : classique sur une piste damée seulement en skating × 1,5 ; skating sur une piste à traces classiques seulement × 3 (le skating abîme les traces) ; sentier raquettes ou piéton sans usage nordique × 3 ; damage inconnu neutre.
- Recherche : coûts cumulés précalculés à chaque station des deux sens de chaque arête (une arête entière coûte une lecture, une arête partielle deux interpolations), Dijkstra avec départ et arrivée au milieu d'une arête.

Chiffres (14-15/11/2026, données OSM du 13/11) :

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Graphe | 189 nœuds, 223 arêtes, 66,7 km, 0 km à sens unique, 5 composantes | 623 nœuds, 741 arêtes, 302,4 km, 90,6 km à sens unique, 13 composantes |
| Profils d'altitude | 223 / 223 arêtes (66,7 km), 110 stations interpolées sur 7 002 | 682 / 741 arêtes (271,0 / 302,4 km : le reste est en Suisse), 476 sur 31 350 |
| Fichier `piste_graph.json` / chargement | 375 Ko / 2-3 ms | 1,1 Mo / 4-7 ms |
| Précalcul des coûts du modèle de temps | 0,8 ms | 3,1 ms |
| Routage, 2 000 requêtes aléatoires (composante principale) | moyenne 2,6 µs, max 13 µs | moyenne 13,5 µs, p99 29 µs, max 32 µs |
| Rattachement au plus proche | 0,5 µs | 0,6 µs |

Itinéraires de preuve (plus court chemin ; longueur égale à la somme des `length_m` OSM des arêtes parcourues à 0,01 % près) :

| Itinéraire | Longueur | D+ / D- | Contrôle d'altitude |
|---|---|---|---|
| Le Laus → refuge des Fonts | 11,36 km | 462 / 145 m | refuge : refuges.info 2 040 m, MNT 2 042 m |
| Le Laus (Cancel) → Brunissard par l'Izoard | 16,62 km | 700 / 591 m | point haut 2 360 m (col d'Izoard 2 360 m) ; fontaine 1 840 / 1 835 m |
| Jeu de test jurassien → Chalet Gaillard | 12,90 km (13,0 km au temps, D+ 360 m au lieu de 413) | 413 / 260 m | chalet 1 232 / 1 235 m |
| Chalet Gaillard → refuge des Prés d'Haut | 20,46 km (retour 21,94 km : sens uniques) | 511 / 484 m | refuge 1 276 / 1 262 m |
| Chalet des Ministres → abri de la Citadelle | 23,42 km | 594 / 537 m | 1 223 / 1 223 m, 1 280 / 1 279 m |

Limites constatées dans les données P2 : sur le jeu de test alpin, la « Liaison Le Laus - Les Fonts » et les pistes du Cancel s'arrêtent à 18 m l'une de l'autre au Laus (connecteurs P2 ≤ 10 m) : deux composantes, pas d'itinéraire Les Fonts → Izoard. Sur le jeu de test jurassien, 41 des 533 nœuds de la composante principale sont hors de sa plus grande composante fortement connexe (sens uniques) : 146 des 2 000 requêtes aléatoires n'ont pas d'itinéraire. Le D+ dépend du pas du profil dans le Jura (village du jeu de test jurassien → Chalet Gaillard : 413 m à 10 m, 378 m à 50 m), peu sur le jeu de test alpin (462 / 455 m).
