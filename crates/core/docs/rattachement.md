# skimap-core, C2 : rattachement et écart

[← skimap-core](../README.md)

HMM de Newson et Krumm : candidats à 100 m au plus (8 au plus) plus un état « hors réseau » ; émission en mélange de deux normales (σ 10 m, 10 % à σ 40 m) ; transition `exp(−|d_réseau − d_vol d'oiseau| / β) / β`, β = 10 m ; distances sur le réseau par Dijkstra borné, tampons réutilisés. `Matcher::push` donne l'état filtré (en ligne), `decode` le chemin de Viterbi sur l'historique gardé (une heure par défaut).

Écart d'itinéraire : distance du point rattaché (ou du fix hors réseau) à l'itinéraire prévu, alerte si > 40 m pendant 15 s, fin si < 15 m pendant 5 s. Mode couloir (hors piste) : distance du fix à la trace prévue, alerte si > 50 m pendant 20 s, fin si < 35 m pendant 5 s.

Traces synthétiques tirées d'itinéraires réels (paires de preuve dans les deux sens et itinéraires aléatoires d'au moins 5 km : 10 itinéraires et 135,7 km à Cervières, 12 et 179,3 km à Chapelle-des-Bois), vitesse du modèle C5 classique, erreur de Gauss-Markov par axe, épisodes « forêt » de 30 m pendant 2 à 10 s (un toutes les 50 s en moyenne). Pour σ 15 m + forêt, 1 fix/s, environ 24 h de trace par site :

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Bonne arête, en ligne / Viterbi | 93,0 / 94,0 % | 92,0 / 92,5 % |
| Erreur médiane / p95 : GPS, en ligne | 19,2 / 42,8 m ; 12,2 / 38,3 m | 19,2 / 42,7 m ; 11,8 / 36,6 m |
| Fausses alertes, itinéraire / couloir | 0,08 / h ; 0 | 0,17 / h ; 0 |
| Mauvais embranchement : détectés, délai médian (max) | 8 / 8, 54 s (64 s) | 11 / 12 (1 alerte avant l'embranchement), 34 s (40 s) |
| Sortie du couloir : détectées, délai médian (max) | 10 / 10, 28 s (81 s) | 12 / 12, 23 s (37 s) |
| Temps par fix (rattachement + suivi), poste | moyenne 1,1 à 1,9 µs, p99 3 à 4 µs (selon les exécutions) | moyenne 0,7 à 1,1 µs, p99 2,0 à 2,8 µs, max 11 µs |

Le balayage du seuil (sortie de l'exemple) a fixé 40 m / 15 s : 20 m / 10 s donne 1,29 fausse alerte par heure, 50 m / 20 s aucune mais 41 à 77 s de délai médian.
