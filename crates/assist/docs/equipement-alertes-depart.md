# A6 équipement (#93), A8 alertes (#95), A9 meilleur moment (#96)

[← skimap-assist](../README.md)

## A6 — liste d'équipement

`equipment::equipment(&SiteData, &Outing)` ; sortie `equipment_<date>.json` et `.txt` ; voix `Event::Equipment` : chaque élément a sa phrase, dont « Des lunettes de soleil et de la crème, le ciel sera dégagé. », « Des moufles et une protection du visage, il fera très froid. » et « Des skis à écailles ou des peaux, le fart de retenue tiendra mal. ». Chaque élément porte le fait qui l'a déclenché :

| Élément | Quand |
|---|---|
| DVA, pelle, sonde | 50 m ou plus en terrain « exposé » ou plus (P16–P18), ou dans une emprise CLPA (P20), ou hors piste avec un BERA ≥ 3 |
| frontale | retour prévu (temps × 1,2) moins d'une heure avant le coucher du soleil, ou départ avant le lever |
| couche chaude ; moufles et protection du visage | ressenti ≤ −10 °C ; ≤ −20 °C |
| batterie externe, prévenir un proche | 1 km ou un quart de l'itinéraire sans aucun réseau (P12) ; prévenir aussi au-delà de 3 h ou hors piste |
| boussole | brouillard prévu pendant la sortie |
| lunettes de soleil et crème | ciel dégagé (< 30 % de nuages) |
| skis à écailles ou peaux | retenue du fart incertaine ce jour (A2) |

Sorties types (exemple `assist --outings --suggest`) : Le Laus → refuge des Fonts sur pistes, Le Laus → col d'Izoard hors piste, traversée du Risoux hors piste (itinéraires de preuve de core, avec leur temps C5 ou C3), et la meilleure boucle de chaque demande A5.

## A8 — alertes de changement de conditions

`alerts::Monitor::update(heure, distance parcourue)` (règles dans la doc du module) : vent qui se lève (rafales ≥ 40 km/h et +20 depuis le départ), brouillard qui arrive, neige qui commence (≥ 0,5 cm/h), température qui chute (−5 °C, ou ressenti −6 °C, en 3 h), heure de demi-tour avancée par un retard (allure observée / temps du modèle, à partir de 10 min et 10 % de retard après 20 min prévues : le point le plus lointain d'où le retour au départ à cette allure, × 1,2, finit avant l'heure limite de C7), « demi-tour maintenant » quand il est passé. Voix : `Event::ConditionChange`, `TurnaroundAdvanced`, `Turnaround`.

Rejeux (`alerts_<date>.json` et `.txt`) : la météo archivée de chaque jour (`day_replay`, sortie de 10 h, retour prévu à 16 h) et la meilleure boucle de chaque demande A5 avec des traces synthétiques (`synthetic_trace`) à l'allure du modèle, 1,5 et 2 fois plus lente, puis un départ tardif (retour prévu 30 min avant l'heure limite) 1,5 fois plus lent.

## A9 — meilleur moment pour partir

`best_time::best_time(point, qualité A3, affluence A4, Request)` : départs toutes les 30 min de 8 h (ou 30 min après le lever) tant que la sortie (temps × 1,2) finit avant l'heure limite C7 ; points sur les heures de la sortie : surface de neige (A3, score moyen − 2), rafales, brouillard, neige, pluie, froid ressenti, soleil, affluence d'un jour chargé entre 10 h 30 et 14 h (pic supposé, sans donnée de fréquentation), retour à moins de 30 min de l'heure limite. Sortie `best_time_<date>.json` et `.txt`, une ligne par jour et par point ; la première entre dans le briefing (texte et `Briefing::BestTime` à la voix). Le texte dit ce que changerait un départ deux heures plus tôt ou plus tard.
