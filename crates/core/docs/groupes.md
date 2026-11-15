# skimap-core, C11 : temps estimé appris par groupe

[← skimap-core](../README.md) · ticket C11 (#130), base de C6 (#38) · suivi et propositions : [skimap-plan](../../plan/README.md)

Le modèle C5 ([temps.md](temps.md)) donne le temps **en mouvement** d'un skieur adulte. Un groupe (« seul », « en famille », « avec X ») va à son propre rythme, s'arrête, joue, fait des allers-retours. Module `calibration` : paramètres par groupe, a priori, ajustement bayésien conjugué sur les sorties passées (hors ligne) et sur la sortie du jour (en ligne, sans allocation), prédiction avec fourchette.

## Modèle

Temps d'un itinéraire = temps en mouvement + temps d'arrêt.

- **Mouvement** : sur chaque classe de terrain (montée, plat, descente : pente au-delà de ± 2 %, les classes de `eta::Breakdown`), le groupe avance à `exp(θ)` fois le rythme C5. `θ` est le rythme de **progression** : il comprend les détours et explorations (distance skiée qui n'avance pas sur l'itinéraire), gardés à part pour l'affichage (`ρ = ln(distance skiée / progression)`) ; la vitesse en mouvement est `θ + ρ`. La classe d'un tronçon se lit sur le profil de temps (rapport des temps aller et retour du modèle, borne à + 2 %), sans altitude.
- **Arrêts** de 90 s ou plus (règle de pause du suivi J3) : processus de Poisson en temps de mouvement, taux `λ` par heure (loi gamma), durées log-normales (normale–inverse-gamma sur le log).

## A priori (`PaceParams::prior`, `prior_row`)

| Groupe (plus jeune enfant) | Vitesse (C5 = 1) | Écart type prédictif (log) | Détours | Arrêts / h | Arrêt médian | Source |
|---|---|---|---|---|---|---|
| adulte, intermédiaire (débutant / confirmé) | 1,0 (0,9 / 1,2) | 0,25 | 3 % | 0,5 | 4 min | hypothèse |
| enfants 14–17 ans | 1,0 × forme | 0,30 | 4 % | 1,5 | 6 min | hypothèse, alignée sur le témoignage |
| 11–13 ans | 1,0 × forme | 0,35 | 5 % | 2,5 | 7 min | témoignage (n = 1) |
| 8–10 ans | 1,0 × forme | 0,35 | 6 % | 3 | 8 min | témoignage (n = 1) |
| 5–7 ans | 0,55 × forme | 0,40 | 12 % | 3,5 | 8 min | hypothèse, prudente |
| moins de 5 ans | 0,30 × forme | 0,45 | 15 % | 4 | 8,5 min | hypothèse, prudente |
| plan sans groupe (`PaceParams::unknown`) | 1,0 | 0,50 | 3 % | 0,5 (poids d'un arrêt) | 4 min | hypothèse |

Le niveau des adultes multiplie aussi les lignes d'enfants, à moitié (débutants : × 0,95). Seule source publiée : la **forme par terrain** des enfants, Sollie et al. 2021 (Scand J Med Sci Sports, PMID 33231327) : des skieurs de compétition de 14 ans sont 13 % plus lents que des adultes, 19 % en montée, 11 % en terrain vallonné, 8 % en descente, soit 0,91 en montée et 1,03 en descente rapportés au plat. **Aucune publication trouvée** sur la vitesse de jeunes enfants en ski de fond de loisir, leurs arrêts ni leurs détours (recherche PubMed, novembre 2026).

### Le témoignage (n = 1, pas une étude)

Recueilli auprès de l'utilisateur le 15/11/2026 (`calibration::TESTIMONY`) : « En famille avec enfants débutants comme nous, on a fait 20 bornes par jour et jusqu'à piste rouge. Ça nous a pris 4 h de ski et 6 h en tout peut-être, avec les pauses et les détours. » Une famille, quelques jours, des durées de mémoire (« peut-être ») : un seul point, d'incertitude large, qui sert à **corriger un excès de prudence**, pas à mesurer. Les anciens a priori (0,60 × C5 de 8 à 10 ans, 0,75 à 11–13, adultes débutants à 0,75) prédisaient pour cette famille, sur les boucles de 19 à 21 km jusqu'au rouge de Chapelle-des-Bois (C5 : 3 h 15 à 3 h 40, 5,8 à 6 km/h), **7 h 45 à 8 h 35 en mouvement et 8 h 40 à 9 h 35 en tout** (médianes), et le matin ne lui proposait en 6 h que des boucles de 7 à 14 km. Le modèle C5 est celui d'un adulte de loisir (5,3 à 6 km/h sur les boucles des deux sites) : le témoignage met la famille débutante avec enfants de 8 ans et plus **à peu près au rythme C5 en mouvement** et **un tiers de la journée à l'arrêt**.

| Boucle (a priori de « Famille avec enfants débutants ») | C5 | Mouvement | Arrêts | Total (médiane, 10–90 %) | Prévu (80 %) |
|---|---|---|---|---|---|
| Chapelle-des-Bois, 19,4 km, D+ 455 m, jusqu'au rouge | 3 h 17 | 4 h 08 | 2 h 00 | **5 h 32** (3 h 06 à 9 h 51) | 8 h 05 |
| Chapelle-des-Bois, 20,9 km, D+ 535 m, une piste noire | 3 h 37 | 4 h 34 | 2 h 11 | 6 h 06 (3 h 26 à 10 h 51) | — |
| Chapelle-des-Bois, 18,2 km, D+ 435 m, jusqu'au rouge | 3 h 03 | 3 h 52 | 1 h 51 | 5 h 09 (2 h 53 à 9 h 11) | 7 h 32 |
| Cervières, 15,8 km, D+ 625 m (pas de boucle de 20 km depuis Le Laus) | 3 h 00 | 3 h 51 | 1 h 50 | 5 h 08 (2 h 53 à 9 h 10) | 7 h 31 |

Test `the_testimony_falls_in_the_prior_range` (`crates/plan/tests/pace.rs`) : sur les boucles de 19 à 21,5 km jusqu'au rouge de Chapelle-des-Bois, mouvement entre 3 h 30 et 4 h 30, total médian entre 5 h 30 et 6 h 30, les 6 h du témoignage dans la fourchette à 80 %. Sous 8 ans, rien n'a été observé : les lignes restent lentes (0,55 et 0,30 × C5).

## Prudence avec des enfants (choix, pas des mesures)

L'utilisateur : « Pas une étude. On doit être prévenants. » Le témoignage rend la journée de 20 km faisable ; il ne doit pas devenir la proposition par défaut. Avec un enfant dans le groupe (`GroupProfile::has_children`) :

- **Planifier sur le côté lent** : `ProfileModel::plan_quantile` = 0,8 (`PLAN_Q_CHILDREN`), le quantile à 80 % du temps total (8 chances sur 10 d'avoir fini), au lieu de la médiane. Il sert au choix des boucles (A5, fenêtre de 75 à 110 % du temps demandé), à l'heure de retour dite et affichée, au demi-tour A8 (`alerts::Monitor::for_model`, profil mis à l'échelle de ce temps) et aux propositions. Sans enfant, rien ne change (médiane, marge de C7 × 1,2). Avec la largeur actuelle des a priori, le côté lent vaut 1,46 fois la médiane : pour 6 h, le matin propose 13 à 14 km à Chapelle-des-Bois et 13 km à Cervières, et **la boucle de 20 km n'apparaît qu'en dernier, comme option « si tout va bien »** (`Suggestion::if_all_goes_well` : elle tient dans le temps demandé à la médiane, pas sur le côté lent). La fourchette affichée garde la médiane et ce côté lent : « médiane 3 h 55, prévu sur 5 h 45 (côté lent : 8 chances sur 10 d'avoir fini), retour prévu vers 14 h 45 ». Le côté lent remplace le × 1,2 de C7 : il est déjà plus large (× 1,46).
- **Une heure de lumière au lieu de 30 min** (`GroupProfile::turnaround_params`, `CHILD_LIGHT_MARGIN_MIN` = 60) : limite de retour et demi-tour plus tôt. Raisons : un enfant fatigué en fin de journée va moins vite que son rythme du matin (le modèle du jour ne le voit qu'après coup) ; le froid tombe avec l'ombre et le crépuscule, plus vite pour un enfant qui transpire puis s'arrête ; un enfant ne se presse pas, et un adulte ne peut pas « finir vite » avec lui.
- **Proposer d'écourter plus tôt** (`ProposeParams::with_children`) : retard de 10 min et 10 % au lieu de 15 et 15 %, tenu 5 min au lieu de 10, à partir de 15 min de sortie au lieu de 20, retard « plus probable que non » (médiane du jour contre médiane prévue) au lieu de 9 chances sur 10. Raisons : la fatigue d'un enfant s'accumule plus vite que ne le dit son rythme passé, et une sortie dépassée ne revient pas. Q9-32 : même rythme, même journée lente, la boucle courte est proposée 16 min après le départ avec des enfants, la première proposition (garder la boucle) vient après 85 min pour des adultes seuls.
- En sortie, le côté lent s'applique à la médiane du jour avec le rapport du matin (côté lent / médiane) : la dispersion du jour part plus large que celle du matin (a priori en ligne plafonnés) et son quantile à 80 % écourterait toute sortie au départ.

Ces valeurs (80 %, 60 min, deux tiers des seuils) sont des **choix de prudence** à revoir sur de vraies sorties (T1, T5), pas des mesures.

## Apprentissage

- **Hors ligne** (mode préparation E2) : une sortie passée est rejouée dans le suivi (`skimap_plan::pace::digest`, même segmentation mouvement / arrêt que pendant la sortie) ; par terrain avec 10 min de mouvement, `y = ln(temps du modèle parcouru / temps réel en mouvement)` met à jour une normale–inverse-gamma (moyenne du groupe et variabilité d'un jour à l'autre, prédictive de Student pour la sortie suivante) ; arrêts : gamma avec les comptes et les heures de mouvement, durées sur le log. **Robuste** : chaque observation est pondérée par le poids de Huber (`min(1, 2 / |z|)`, `z` résidu standardisé contre la prédictive), mise à jour conjuguée pondérée ; une sortie aberrante (skieur blessé, rythme / 3) pèse 0,23.
- **En ligne** (`DayPace`, taille fixe) : effet du jour `δ` commun aux terrains, a priori `N(0, V₀)` (variance prédictive du groupe), observé par le rapport temps prédit au rythme du groupe / temps réel en mouvement, sommes oubliées sur une heure de mouvement (le rythme dérive : un enfant qui fatigue), bruit de 0,2 en log pour 10 min ; arrêts du jour sur les a priori du groupe plafonnés à 1 h et 3 arrêts de poids.
- **Prédiction** : mouvement log-normal (terrains corrélés), arrêts en somme de Poisson composée avec l'incertitude de `λ` (plus 30 % de variabilité d'un jour à l'autre, hypothèse) et de la durée, durée résiduelle d'un arrêt en cours ; total ramené à une log-normale, **fourchette à 80 %**.

Coût (poste, release) : ajustement sur 8 sorties 0,2 à 1,4 µs ; résumé d'une sortie (rejeu de 3 000 à 6 000 entrées dans le suivi) 1,0 à 3,3 ms.

## Preuve (traces synthétiques, `cargo run --release -p skimap-plan --example pace_eval`)

Trois groupes simulés (`scenario::Behaviour`) sur les deux sites, 8 jours chacun, rythme, arrêts et détours tirés d'un jour à l'autre ; famille (6 et 9 ans : 0,55 à 0,65 de la vitesse C5, 3 arrêts / h de 4 min, 4 allers-retours / h de 30 m) sur une boucle de 70 min au modèle, adultes sur la première boucle du site. Erreur moyenne de l'heure d'arrivée (min), jours appris sur au moins 3 sorties précédentes (validation croisée : apprendre sur les N sorties d'avant, tester sur la suivante) ; a priori recalés et propositions plus tôt avec des enfants (15/11/2026, les jours comptés changent avec les bascules) :

| Site, groupe | Matin : C5 seul / C11 a priori / C11 appris | 25 % : C5 / C11 | 50 % : C5 / rythme extrapolé / C11 | 75 % : C5 / C11 |
|---|---|---|---|---|
| Cervières, famille | 110 / 48 / 29 | 73 / 24 | 30 / 7 / 9 | 5 / 6 |
| Chapelle-des-Bois, famille | 110 / 42 / 22 | 81 / 15 | 55 / 19 / 11 | 24 / 4 |
| Cervières, adulte rapide | 47 / 60 / 23 | 24 / 3 | 3 / 4 / 1 | 1 / 1 |
| Chapelle-des-Bois, adulte rapide | 30 / 39 / 16 | 20 / 4 | 11 / 1 / 2 | 6 / 1 |
| Cervières, adulte normal | 24 / 24 / 25 | 14 / 8 | 9 / 8 / 8 | 3 / 2 |
| Chapelle-des-Bois, adulte normal | 16 / 17 / 18 | 12 / 9 | 6 / 1 / 5 | 2 / 1 |

Le gain est là où C5 se trompe (famille, adulte rapide) ; un adulte au rythme du modèle n'y gagne rien le matin (C5 est juste en moyenne, la variabilité du jour domine). L'a priori des 5–7 ans (0,55 × C5, 3,5 arrêts / h de 8 min) est plus lent que cette famille simulée (médiane a priori 3 h 40 à 3 h 50 pour des durées réelles de 2 h 20 à 3 h 40) : c'est voulu sous 8 ans, sans observation. Fourchette du matin de la famille : 1 h 55 à 7 h 05 a priori (médiane 3 h 40, prévu sur 5 h 40), 2 h 00 à 3 h 55 après 7 sorties (médiane 2 h 45, prévu sur 3 h 30) ; couverture à 80 % de la fourchette apprise : 75 à 100 % (famille, adulte rapide), 40 à 100 % (adulte normal).

## Limites

- **Validation réelle en attente** : traces synthétiques seulement ; le critère du ticket (erreur réduite sur de vraies sorties par profil, adulte seul et famille) attend de vraies sorties (T1, C6).
- A priori des enfants : un seul témoignage (n = 1, 8 ans et plus) et des hypothèses (forme par terrain publiée) ; seuils de prudence avec des enfants (80 %, une heure de lumière, propositions plus tôt) : des choix ; sans compteur de pas, un aller-retour d'enfant de moins de 40 m compte comme arrêt (règle de pause J3), de façon cohérente hors ligne et en ligne ; avec les pas comptés par le téléphone (X10), c'est du mouvement (un détour), et les arrêts sont datés du début et de la fin des pas.
- Paramètres du suivi en ligne (bruit 0,2, oubli 1 h, plafonds) : hypothèses à régler sur le terrain (T5).
