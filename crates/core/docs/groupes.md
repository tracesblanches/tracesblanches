# skimap-core, C11 : temps estimé appris par groupe

[← skimap-core](../README.md) · ticket C11 (#130), base de C6 (#38) · suivi et propositions : [skimap-plan](../../plan/README.md)

Le modèle C5 ([temps.md](temps.md)) donne le temps **en mouvement** d'un skieur adulte. Un groupe (« seul », « en famille », « avec X ») va à son propre rythme, s'arrête, joue, fait des allers-retours. Module `calibration` : paramètres par groupe, a priori, ajustement bayésien conjugué sur les sorties passées (hors ligne) et sur la sortie du jour (en ligne, sans allocation), prédiction avec fourchette.

## Modèle

Temps d'un itinéraire = temps en mouvement + temps d'arrêt.

- **Mouvement** : sur chaque classe de terrain (montée, plat, descente : pente au-delà de ± 2 %, les classes de `eta::Breakdown`), le groupe avance à `exp(θ)` fois le rythme C5. `θ` est le rythme de **progression** : il comprend les détours et explorations (distance skiée qui n'avance pas sur l'itinéraire), gardés à part pour l'affichage (`ρ = ln(distance skiée / progression)`) ; la vitesse en mouvement est `θ + ρ`. La classe d'un tronçon se lit sur le profil de temps (rapport des temps aller et retour du modèle, borne à + 2 %), sans altitude.
- **Arrêts** de 90 s ou plus (règle de pause du suivi J3) : processus de Poisson en temps de mouvement, taux `λ` par heure (loi gamma), durées log-normales (normale–inverse-gamma sur le log).

## A priori (`PaceParams::prior`)

| Groupe (plus jeune enfant) | Vitesse (C5 = 1) | Écart type prédictif (log) | Détours | Arrêts / h | Arrêt médian |
|---|---|---|---|---|---|
| adulte, intermédiaire (débutant / confirmé) | 1,0 (0,75 / 1,25) | 0,25 | 3 % | 0,5 | 4 min |
| enfants 14–17 ans | 0,85 × forme | 0,30 | 4 % | 1 | 4 min |
| 11–13 ans | 0,75 × forme | 0,35 | 6 % | 1,5 | 4 min |
| 8–10 ans | 0,60 × forme | 0,35 | 10 % | 2 | 5 min |
| 5–7 ans | 0,45 × forme | 0,35 | 15 % | 3 | 5 min |
| moins de 5 ans | 0,30 × forme | 0,40 | 15 % | 4 | 6 min |
| plan sans groupe (`PaceParams::unknown`) | 1,0 | 0,50 | 3 % | 0,5 (poids d'un arrêt) | 4 min |

Source unique : **forme par terrain** des enfants, Sollie et al. 2021 (Scand J Med Sci Sports, PMID 33231327) : des skieurs de compétition de 14 ans sont 13 % plus lents que des adultes, 19 % en montée, 11 % en terrain vallonné, 8 % en descente, soit 0,91 en montée et 1,03 en descente rapportés au plat. **Aucune publication trouvée** sur la vitesse de jeunes enfants en ski de fond de loisir, leurs arrêts ni leurs détours (recherche PubMed, novembre 2026) : toutes les autres valeurs sont des **hypothèses**, volontairement larges (facteur 1,4 à 1 écart type), à remplacer par les sorties du groupe.

## Apprentissage

- **Hors ligne** (mode préparation E2) : une sortie passée est rejouée dans le suivi (`skimap_plan::pace::digest`, même segmentation mouvement / arrêt que pendant la sortie) ; par terrain avec 10 min de mouvement, `y = ln(temps du modèle parcouru / temps réel en mouvement)` met à jour une normale–inverse-gamma (moyenne du groupe et variabilité d'un jour à l'autre, prédictive de Student pour la sortie suivante) ; arrêts : gamma avec les comptes et les heures de mouvement, durées sur le log. **Robuste** : chaque observation est pondérée par le poids de Huber (`min(1, 2 / |z|)`, `z` résidu standardisé contre la prédictive), mise à jour conjuguée pondérée ; une sortie aberrante (skieur blessé, rythme / 3) pèse 0,23.
- **En ligne** (`DayPace`, taille fixe) : effet du jour `δ` commun aux terrains, a priori `N(0, V₀)` (variance prédictive du groupe), observé par le rapport temps prédit au rythme du groupe / temps réel en mouvement, sommes oubliées sur une heure de mouvement (le rythme dérive : un enfant qui fatigue), bruit de 0,2 en log pour 10 min ; arrêts du jour sur les a priori du groupe plafonnés à 1 h et 3 arrêts de poids.
- **Prédiction** : mouvement log-normal (terrains corrélés), arrêts en somme de Poisson composée avec l'incertitude de `λ` (plus 30 % de variabilité d'un jour à l'autre, hypothèse) et de la durée, durée résiduelle d'un arrêt en cours ; total ramené à une log-normale, **fourchette à 80 %**.

Coût (poste, release) : ajustement sur 8 sorties 0,2 à 1,4 µs ; résumé d'une sortie (rejeu de 3 000 à 6 000 entrées dans le suivi) 1,0 à 3,3 ms.

## Preuve (traces synthétiques, `cargo run --release -p skimap-plan --example pace_eval`)

Trois groupes simulés (`scenario::Behaviour`) sur les deux sites, 8 jours chacun, rythme, arrêts et détours tirés d'un jour à l'autre ; famille (6 et 9 ans : 0,55 à 0,65 de la vitesse C5, 3 arrêts / h de 4 min, 4 allers-retours / h de 30 m) sur une boucle de 70 min au modèle, adultes sur la première boucle du site. Erreur moyenne de l'heure d'arrivée (min), jours appris sur au moins 3 sorties précédentes (validation croisée : apprendre sur les N sorties d'avant, tester sur la suivante) :

| Site, groupe | Matin : C5 seul / C11 appris | 25 % : C5 / C11 | 50 % : C5 / rythme extrapolé / C11 | 75 % : C5 / C11 |
|---|---|---|---|---|
| Cervières, famille | 116 / 28 | 76 / 24 | 30 / 7 / 8 | 5 / 5 |
| Chapelle-des-Bois, famille | 102 / 23 | 76 / 14 | 50 / 17 / 11 | 21 / 4 |
| Cervières, adulte rapide | 47 / 23 | 24 / 3 | 3 / 4 / 1 | 1 / 1 |
| Chapelle-des-Bois, adulte rapide | 30 / 16 | 20 / 4 | 11 / 1 / 2 | 6 / 1 |
| Cervières, adulte normal | 24 / 25 | 14 / 8 | 9 / 8 / 8 | 3 / 2 |
| Chapelle-des-Bois, adulte normal | 16 / 18 | 12 / 9 | 6 / 1 / 5 | 2 / 1 |

Le gain est là où C5 se trompe (famille, adulte rapide) ; un adulte au rythme du modèle n'y gagne rien le matin (C5 est juste en moyenne, la variabilité du jour domine). Fourchette du matin de la famille : 2 h 20 à 6 h 40 a priori, 2 h 00 à 4 h 05 après 7 sorties (durées réelles 2 h 20 à 3 h 20) ; couverture à 80 % de la fourchette apprise : 80 à 100 % (famille, adulte rapide), 40 à 75 % (adulte normal, 5 à 8 jours : trop étroite).

## Limites

- **Validation réelle en attente** : traces synthétiques seulement ; le critère du ticket (erreur réduite sur de vraies sorties par profil, adulte seul et famille) attend de vraies sorties (T1, C6).
- A priori des enfants : hypothèses sans source (sauf la forme par terrain) ; un aller-retour d'enfant de moins de 40 m peut compter comme arrêt (règle de pause J3), de façon cohérente hors ligne et en ligne.
- Paramètres du suivi en ligne (bruit 0,2, oubli 1 h, plafonds) : hypothèses à régler sur le terrain (T5).
