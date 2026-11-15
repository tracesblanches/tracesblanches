# skimap-core, C5 : temps estimé

[← skimap-core](../README.md)

Bilan de puissance du rapport 06 : `P = v·m g (sin α + μ cos α) + ½ ρ CdA v³`, résolu en `v` par Newton (fonction convexe), vitesse plafonnée en descente. Skating = classique × (1 + gain(pente)), gain de Bolger 2015 : 13,5 % en montée, 10,5 % sur le plat, 1,5 % en descente (interpolé entre −5 %, −1 %, +1 %, +5 %). Densité de l'air selon l'altitude et la température.

| Paramètre (`SkierParams`, `Conditions`) | Défaut | Source |
|---|---|---|
| masse avec matériel | 75 kg | hypothèse, par utilisateur |
| puissance propulsive | 70 W | hypothèse : plat classique 10,6 km/h, 6 % : 3,8 km/h (neige damée, −5 °C) ; à calibrer (C6) |
| CdA | 0,5 m² | hypothèse, skieur debout |
| vitesse maximale | 7 m/s | hypothèse, descente de loisir |
| μ | damée sèche : 0,070 à −25 °C, 0,050 à −15, 0,035 à −8, 0,025 à −3, 0,030 à 0 ; mouillée 0,045 ; neuve × 1,4 ; glacée 0,025 ; hors damage ≥ 0,08 | forme d'après Colbeck 1994 et Almqvist 2022 (rapport 06), valeurs à calibrer |
| facteur de friction, température, classe de neige | 1, −5 °C, damée | par utilisateur et par jour |

Contrôles : gain global du skating sur un parcours aux parts de temps de Bolger (55 / 17,5 / 27,5 %) : 9,7 % (Bolger : 9 à 11 %). +10 % de friction sur un parcours de course (25 km/h, 55 % du temps en montée) : +3,0 % de temps (Moxnes 2013 : +3 à 4 %) ; +10 % de traînée : +1,6 % (plus faible que Moxnes).

Comparaison sur les mêmes itinéraires (classique et skating au modèle par défaut) :

| Itinéraire | Classique | Skating | Sans relief | Munter (4 / 10) | Tobler | Skitourenguru (régression du rapport 06) |
|---|---|---|---|---|---|---|
| Le Laus → Fonts | 2h15 | 2h00 | 1h04 | 3h52 | 2h37 | 4h14 |
| Fonts → Le Laus | 1h06 | 1h00 | 1h04 | 2h48 | 2h19 | 3h34 |
| Le Laus → Brunissard (Izoard) | 3h09 | 2h49 | 1h33 | 5h29 | 3h49 | 6h15 |
| Chapelle → Chalet Gaillard | 2h11 | 1h57 | 1h16 | 3h56 | 2h49 | 4h34 |
| Chalet Gaillard → Chapelle | 1h37 | 1h27 | 1h16 | 3h27 | 2h41 | 4h15 |
| Chalet des Ministres → Citadelle | 3h23 | 3h02 | 2h13 | 6h46 | 5h01 | 7h59 |

Munter, Tobler et Skitourenguru sont des formules de randonnée ou de ski de randonnée : plus lentes que le ski de fond damé, sans distinction de technique. L'erreur sur des traces réelles reste à mesurer (critère du ticket, quand T1 sera disponible).

Temps d'arrêt, rythme propre à chaque groupe et apprentissage sur ses sorties : [C11, temps appris par groupe](groupes.md).
