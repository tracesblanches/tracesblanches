# skimap-srndiff, SR1 : calibrage, accord, relecture, coût

[← skimap-srndiff](../README.md)

Mesures du 15/11/2026 (`srndiff refs`, `profiles`, `roads`, `evaluate`, `review`, `bench`), poste Intel Core Ultra 7 265K partagé avec d'autres compilations (charge moyenne de 60 à 140 sur 20 cœurs : les temps sont des majorants).

## Données

- **124 tracés profilés** (`references.json`) : 87 itinéraires de la couche G5 (SRN et ski de randonnée facile d'au moins 1 km, tous les itinéraires cotés), 49 topos français de skirandonneenordique.com avec GPX, 4 itinéraires hors piste calculés par C3 ; profil tous les 10 m.
- **Relief et canopée** : dalles LiDAR HD à moins de 1,5 km du tracé seulement (GTJ : 315 dalles de 1 km² au lieu des quelque 1 770 de son emprise de 33 × 53 km) ; **exposition aux avalanches** : modèle de la crate avalanche sur ce couloir, ou couche du site (CLPA comprises) sur les deux jeux de test (alpin et jurassien) ; **réseau et zones** : couches des deux sites (ailleurs « non évalué ») ; **sorties** : routes ouvertes l'hiver de la BD TOPO (les tronçons à `periode_de_fermeture` « Hiver », comme la route de l'Izoard au-dessus du Laus, sont écartés). Overpass, premier choix, n'a plus répondu ce jour-là.
- **Contrôle des profils** : longueur calculée contre longueur publiée, écart médian −0,1 % (absolu 1,6 %) ; dénivelé positif, +2,9 % (absolu 8,5 %) sur les tracés qui publient ces chiffres.
- **Cotations** : 62 cotations ramenées aux niveaux SRN sur 44 itinéraires distincts (groupes : deux tracés dont l'un suit l'autre sur au moins la moitié de sa longueur, toutes sources confondues) : skirandonneenordique.com 49 (Facile 13, Moyenne 31, Difficile 5), PNR du Vercors 9 (niveau 1 : 3, 2 : 4, 4 : 2), OSM `piste:difficulty` 4 ; Camptocamp 27 (ski 1.1 à 1.3), échelle de ski de randonnée, traitée à part.

## Accord avec les cotations publiées

« Difficile » de skirandonneenordique.com couvre nos niveaux difficile et très difficile ; « toutes » ramène les trois échelles à 3 niveaux. Matrices : lignes = publié (facile → difficile), colonnes = calculé. Kappa pondéré linéairement.

| Règles | Échelle | n | Exact | ±1 niveau | Écart moyen | Kappa | Matrice |
|---|---|---|---|---|---|---|---|
| échelle 2025 lue à la lettre (`Params::scale`) | toutes | 62 | 14,5 % | 77,4 % | 1,08 | 0,01 | [[0,3,14],[0,0,36],[0,0,9]] |
| calibrées, **validation en laissant un itinéraire de côté** | toutes | 62 | **58,1 %** | **100 %** | **0,42** | **0,24** | [[1,16,0],[0,30,6],[0,4,5]] |
| idem | skirandonneenordique.com | 49 | 59,2 % | 100 % | 0,41 | 0,22 | [[1,12,0],[0,25,6],[0,2,3]] |
| idem | PNR du Vercors (4 niveaux) | 9 | 66,7 % | 100 % | 0,33 | 0,67 | [[0,3,0,0],[0,4,0,0],[0,0,0,0],[0,0,0,2]] |
| idem | OSM | 4 | 25 % | 50 % | 1,25 | 0,00 | [[0,1,0,0],[0,1,0,0],[0,0,0,0],[0,2,0,0]] |
| analyse : mêmes règles **sans** le plancher avalanche | toutes | 62 | 58,1 % | 95,2 % | 0,47 | 0,31 | [[8,9,0],[7,23,6],[3,1,5]] |

Lecture :

- La validation laisse de côté, tour à tour, chacun des 44 itinéraires (toutes ses cotations) et choisit les paramètres sur les 43 autres : le même jeu est choisi 44 fois sur 44, la validation égale donc l'ajustement sur tout (aucun sur-apprentissage mesurable, mais 62 cotations seulement).
- Toujours répondre « moyen » donnerait aussi 58 % d'accord exact (36 cotations sur 62 sont « moyen ») : le kappa (0,24) est la mesure honnête, un accord faible.
- **Le plancher avalanche explique l'essentiel du désaccord sur « facile »** : 16 des 17 itinéraires publiés faciles traversent du terrain que le modèle dit « exposé » ; sans ce plancher, 8 seraient « facile » et le kappa passerait à 0,31. Ce plancher est une exigence du ticket (« jamais facile à travers une zone exposée ») et reste appliqué ; la classe « exposé » de la crate avalanche est large (toute cellule à moins de 10 m d'un écoulement du scénario rare, α = 18°).
- **Les sources ne s'accordent pas entre elles** : 5 itinéraires cotés par deux sources (chaque tracé à au moins 50 % sur l'autre) : 2 accords seulement (Pont Chabert – Fessole : niveau 1 / Facile ; Moucherotte : niveau 2 / Moyenne) ; Font d'Urle et Pionniers – Ambel : niveau 1 / Moyenne ; Corrençon – Col du Rousset : niveau 4 / Moyenne (le PNR cote la traversée de deux jours, skirandonneenordique.com la technique d'une journée). Notre « très difficile » pour les traversées de plusieurs jours suit l'échelle 2025 (SRN4 = raid) ; le niveau technique est donné à part.
- **Camptocamp** (ski de randonnée, 27 itinéraires) : niveau calculé moyen 2,0 / 2,31 / 2,4 et pente la plus raide moyenne 15,8° / 18,7° / 19,3° pour ski 1.1 / 1.2 / 1.3 : l'ordre est respecté.

## Paramètres calibrés (`Params::default`)

Grille de 9 216 jeux (`pipeline::eval::grid`), critère : écart moyen en niveaux. Choix :

| Paramètre | Valeur | Ce que cela dit |
|---|---|---|
| fenêtre de la pente du tracé | **200 m** (grille 30, 50, 100, 200) | les auteurs cotent la pente d'ensemble d'un passage, pas un ressaut de 30 m ; les ressauts courts restent dans les conseils (fenêtre de 50 m) |
| bornes en terrain ouvert | **13,5 / 18,5 / 23,5°** (échelle −1,5°) | la pente moyennée sur 200 m sous-estime le passage le plus raide qu'un auteur retient |
| forêt dense | **non comptée** dans le niveau (grille : aucune, 70 %, 90 %, bornes 8–15° ou 12–20°) | la détection « forêt dense sans trouée » sur un tracé GPS de quelques mètres d'erreur classe trop de chemins forestiers en forêt ; elle reste un conseil |
| dévers | **non compté** (grille : aucun, 25–30°, 28–33°) | la pente du terrain à 5 m sous une route creusée dans un versant est celle du talus ; reste un conseil (« versant raide traversé ») |
| effort | **bornes de l'échelle × 2** (grille : ×1, ×1,5, ×2, raid seul) ; raid au-delà de 30 km ou 10 h | les topos cotent la technique ; l'effort ne compte que pour les longues journées |
| segments approximatifs | **plus de 40 m** (grille 40, 60, 100, aucun) | 27 des 124 tracés ont des points espacés de plus de 60 m en moyenne (tracés dessinés ou simplifiés) : l'altitude est interpolée entre leurs points |
| plancher « très exposé » | **moyen** (grille : moyen, difficile) | « difficile » sur 10 m de « très exposé » mal placé par le GPS faisait chuter l'accord ; jamais « facile » dans tous les cas |

Répartition des 124 tracés (paramètres calibrés, sens du tracé) : 4 faciles, 102 moyens, 6 difficiles, 12 très difficiles (dont les traversées de plusieurs jours : GTJ, Hauts-Plateaux, Haute Route des Escartons, Champsaur, Haut-Bugey).

## Relecture à la main

21 tracés, textes complets et remarques : [relecture.md](relecture.md).

## Coût de calcul : la GTJ (99,8 km, 9 982 échantillons)

| Étape | Temps | Mesure |
|---|---|---|
| téléphone : échantillonnage sur les rasters SKR1 en mémoire (altitude, pente, exposition, pistes, 25 lectures de canopée) | 1,3 à 4,7 µs par échantillon, soit 13 à 47 ms pour la GTJ | `bench` : 2 839 échantillons de la GTJ dans la grille du jeu de test jurassien, médiane de 20 passages, charge 60 à 143 |
| téléphone : mesures des deux sens (niveau et conseils), règles, conseils, texte | 12 à 43 ms (médiane), 32 à 129 ms au pire | `bench`, profil complet |
| **téléphone, tracé entier** | **25 à 73 ms** | somme ; Pixel 8 non mesuré (téléphone non branché) |
| pipeline, dalles déjà en cache | 186,6 s, dont 177,4 s de modèle d'avalanche (26,2 M cellules, 15 tronçons sur 20 ; les 5 autres lisent la couche du jeu de test jurassien), 7,3 s de lecture de 315 + 315 dalles | `bench --pipeline` |
| pipeline, premier passage (téléchargement compris) | 788 s, dont 635 s de modèle et 153 s de dalles | `profiles` |

Le cas qui domine est le modèle d'exposition du couloir, côté pipeline ; sur le téléphone le calcul complet d'un tracé de 100 km reste sous 0,1 s sur le poste.

## Limites

- Accord modeste (kappa 0,24) : peu de cotations (62), des sources qui se contredisent, et une majorité de « moyen » dans les cotations comme dans nos résultats ; « facile » n'est presque jamais rendu à cause du plancher avalanche.
- Exposition : modèle sur un couloir de 1,5 km (une zone de départ plus lointaine n'est pas vue) ; au bord de la couverture LiDAR HD et hors de France, exposition et relief inconnus : jamais « facile » alors (La Dôle : 42 % du tracé en Suisse).
- Positions : erreurs des tracés publiés (points espacés jusqu'à 429 m en moyenne, tracés à flanc de falaise : 84° de pente sous la Corniche Nord du Vercors) ; les valeurs au-delà de 45° signalent un tracé décalé, pas un passage réel.
- Réseau et zones protégées : seulement sur les deux sites ; ailleurs « non évalué ».
- Canopée : pas encore dans les rasters SKR1 du téléphone (demande à l'orchestrateur).
- Seuils de peaux (8 et 12°), d'isolement (5 km), de réseau (2 km), de neige fraîche (20 cm, +30 % de temps) : déduits, non calibrés faute de cotations qui les isolent.
