# skimap-core, C2 : rattachement et écart

[← skimap-core](../README.md)

HMM de Newson et Krumm : candidats à 100 m au plus (8 au plus), un par passage d'une arête près du fix (minimum local de la distance le long de l'arête : une arête en épingle ou tracée en aller-retour sur elle-même donne ses deux passages), plus un état « hors réseau » ; émission en mélange de deux normales (σ 10 m, 10 % à σ 40 m) ; transition `exp(−|d_réseau − d_vol d'oiseau| / β) / β`, β = 10 m ; distances sur le réseau par Dijkstra borné, tampons réutilisés. `Matcher::push` donne l'état filtré (en ligne), `decode` le chemin de Viterbi sur l'historique gardé (une heure par défaut).

Écart d'itinéraire : distance du point rattaché (ou du fix hors réseau) à l'itinéraire prévu, alerte si > 40 m pendant 15 s, fin si < 15 m pendant 5 s. Mode couloir (hors piste) : distance du fix à la trace prévue, alerte si > 50 m pendant 20 s, fin si < 35 m pendant 5 s.

Traces synthétiques tirées d'itinéraires réels (paires de preuve dans les deux sens et itinéraires aléatoires d'au moins 5 km : 10 itinéraires et 135,7 km sur le jeu de test alpin, 12 et 213,3 km sur le jeu de test jurassien), vitesse du modèle C5 classique, erreur de Gauss-Markov par axe, épisodes « forêt » de 30 m pendant 2 à 10 s (un toutes les 50 s en moyenne). Pour σ 15 m + forêt, 1 fix/s, 24,8 et 28,6 h de trace (15/11/2026, après la correction des candidats ci-dessous) :

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Bonne arête, en ligne / Viterbi | 95,0 / 95,3 % | 93,4 / 93,9 % |
| Hors réseau (en ligne) | 0,10 % | 0,06 % |
| Erreur médiane / p95 : GPS, en ligne | 19,2 / 42,8 m ; 12,0 / 37,5 m | 19,2 / 42,6 m ; 11,5 / 35,3 m |
| Fausses alertes, itinéraire / couloir | 0,08 / h ; 0 | 0,10 / h ; 0 |
| Mauvais embranchement : détectés, délai médian (max) | 8 / 8, 54 s (64 s) | 11 / 12 (1 alerte avant l'embranchement), 32 s (59 s) |
| Sortie du couloir : détectées, délai médian (max) | 10 / 10, 28 s (81 s) | 12 / 12, 21 s (30 s) |
| Temps par fix (rattachement + suivi), poste | moyenne 2,5 µs, p99 6,2 µs | moyenne 1,1 µs, p99 3,1 µs |

Le balayage du seuil (sortie de l'exemple) a fixé 40 m / 15 s : 20 m / 10 s donne 1,13 fausse alerte par heure sur le jeu de test alpin (1,29 avant la correction des candidats), 50 m / 20 s aucune mais 41 à 77 s de délai médian.

Correction du 15/11/2026 (Q3, #103, trouvée par le test de propriétés `trace_on_a_piste_is_never_off_the_network`) : le rattachement ne gardait que le point le plus proche de chaque arête. Sur une arête qui passe deux fois près du fix (épingle, chemin tracé en aller-retour), le passage réel pouvait manquer, la transition devenait impossible et le fix partait « hors réseau » alors qu'il était exactement sur la piste. Avec un candidat par passage, sur les mêmes traces (σ 15 m + forêt, 1 fix/s) : hors réseau 1,89 → 0,10 % sur le jeu de test alpin et 0,10 → 0,06 % sur le jeu de test jurassien ; bonne arête en ligne 93,0 → 95,0 % et 93,3 → 93,4 % ; à 1 fix / 5 s sur le jeu de test alpin, hors réseau 4,04 → 0,12 %, bonne arête 91,5 → 95,7 %. Temps moyen par fix 1,7 → 2,5 µs sur le jeu de test alpin (plus de candidats dans les épingles), 0,9 → 1,1 µs sur le jeu de test jurassien.
