# A5 — suggestion d'itinéraire (#92)

[← skimap-assist](../README.md)

`suggest::suggest(&SiteData, &Request)` : « 2 h, skating, retour avant 16 h » → boucles classées. Sorties : `suggestions_<date>.json`, `.txt` et `.geojson` (WGS84, propriétés `rank`, `label`, `names`, `length_km`, `gain_m`, `eta_min`, `arrival`, `out_and_back_share`, `score`, `reasons`) pour le visualiseur.

Méthode, sur le graphe de C1 (`routes/piste_graph.json`, sens uniques respectés) et le temps de C5 (`EtaModel` : technique, température de l'heure de départ, classe de neige tirée de A3) :

1. départ rattaché au réseau (300 m au plus) ;
2. chaque nœud est un point de demi-tour possible si l'aller le plus rapide plus le retour le plus rapide font 50 à 110 % du temps demandé ; 40 au plus, répartis par échantillonnage du point le plus éloigné ;
3. retour le plus rapide avec les ways OSM de l'aller rendus 4 fois plus chers ; s'il repasse encore sur des tronçons de l'aller (5 % de la boucle ou plus), on cherche aussi le retour avec ces ways 30 fois plus chers : une vraie boucle (un cycle) dès que le réseau en a une ; si aucune boucle simple ne tient dans le temps, des boucles en huit : deux vraies boucles depuis le départ (points de demi-tour à 15–55 % du temps, 20 au plus), aux tronçons distincts, dont les temps font 60 à 125 % du temps demandé (les 20 plus proches) ;
4. niveau : les pistes plus difficiles que demandé coûtent 4 fois plus (`difficulty`, la couleur la plus facile des relations d'un way) ;
5. on garde les boucles de 75 à 110 % du temps demandé qui finissent (temps du modèle × 1,2, marge de C7) avant l'heure de retour demandée et l'heure limite de C7 ;
6. les vraies boucles (moins d'un quart de la longueur parcouru deux fois : une « queue » de départ jusqu'à un huitième de la longueur reste une boucle) sont proposées d'abord, les simples avant les huit (dits comme tels, avec la distance à laquelle on repasse au départ) ; les allers-retours seulement quand aucune vraie boucle ne tient, et c'est dit, avec la plus longue vraie boucle trouvée ; les quasi-doublons (plus de 60 % d'arêtes communes) gardent la meilleure.

Points : écart au temps demandé (−2 × écart relatif), part d'aller-retour (−4 × part, longueur sur des tronçons du graphe déjà parcourus : un way OSM découpé en plusieurs arêtes et parcouru une fois ne compte pas deux fois), pistes fermées (−10 × part) ou ouvertes (+1 × part) d'après les sources du jour (ENJ par la géométrie, à 30 m ; Nordic France par les voies OSM que la couche `status` lui rattache, `osm_way_ids` de `status.geojson`), neige comme A1, terrain exposé aux avalanches (couche P16–P18 : −2 avec un BERA ≥ 3, −0,5 sans BERA ou BERA indisponible), météo des heures de la sortie comme A1, pistes plus difficiles que demandé (−2 × part), pistes nommées (+0,1 chacune, +0,5 au plus).

Limites du graphe, dites et non masquées : nœuds non atteignables depuis le départ, points de demi-tour sans retour possible. Au 15/11/2026 (graphe P2 refermé hors de l'emprise) : Jeu de test alpin 65 nœuds sur 189 (26,8 km) depuis Le Laus et 138 depuis Les Fonts, inchangé, la « Liaison Le Laus - Les Fonts » s'arrêtant sur la D 902 à 17,6 m de la piste du Laus (trou des données OSM, `crates/osm/docs/connexite.md`) ; Jeu de test jurassien 33 sur 649 (14,5 km) depuis le village, contre 96 sur 623 avant la fermeture du réseau hors de l'emprise. Avec les corrections OSM proposées (simulées sur le graphe) : Jeu de test alpin 14 sur 189 depuis chacun des deux départs ; jeu de test jurassien 29 sur 649 (28 dans des réseaux séparés, plus l'entrée à sens unique 2204592763).

Temps : 0,78 s pour trois demandes sur le jeu de test jurassien (649 nœuds : un itinéraire vers chaque nœud et retour, 40 précalculs de coûts), 0,1 s sur le jeu de test alpin. Avec X16 (release, 20 cœurs chargés, 36 demandes) : 0,21 s par demande sur le jeu de test jurassien (0,27 s avant, même machine), 0,04 s sur le jeu de test alpin (0,02 s) ; le retour « 30 fois » n'est cherché que pour les points de demi-tour dont le retour repasse sur l'aller, les huit seulement sans boucle simple (chaque routeur précalcule le coût de toutes les arêtes : c'est ce qui domine).

## Allers-retours (X16, #150)

Avant X16, la part d'aller-retour était comptée par way OSM : un way découpé en plusieurs arêtes du graphe et parcouru une seule fois comptait comme parcouru deux fois (« 17 % » pour une boucle du jeu de test jurassien qui ne repasse nulle part), et le classement ne la pénalisait que de −2 × part. Elle est maintenant mesurée par tronçon (recouvrement des intervalles parcourus sur chaque arête), pénalisée de −4 × part, et les vraies boucles passent avant les allers-retours.

Mesure (`full_data_personal_out_and_back_share_of_the_loops`, météo archivée du 09/01/2026 sur le jeu de test alpin et du 22/02/2026 sur le jeu de test jurassien ; les trois demandes de `presets` et, depuis chaque départ, 60, 90, 120, 180 et 240 min en classique et en skating ; part mesurée dans le test sur les tronçons, indépendamment de l'assistant) :

| Site (demandes) | Avant : sans boucle | Avant : 1re boucle, toutes | Après : sans boucle | Après : 1re boucle, toutes | Boucles ≥ 25 % avant → après |
|---|---|---|---|---|---|
| Jeu de test alpin (23) | 3 | 27,6 %, 29,8 % | 0 | 17,1 %, 26,6 % | 8 → 10 |
| Jeu de test jurassien (13) | 0 | 0,6 %, 1,1 % | 0 | 0,8 %, 1,3 % | 0 → 0 |

Jeu de test alpin, depuis Le Laus : 2 h de skating, 9,6 km dont 15 % repassés (avant : 11,0 km à 50 %) ; 1 h et 1 h 30 en skating, 1 h en classique, sans boucle avant, ont maintenant une vraie boucle (6,5 km à 1 %, 9,2 km à 12 %, 5,1 km à 22 %) ; 2 h en classique, 9,1 km à 10 % (10,7 km à 47 %). Pour 3 h et 4 h, le réseau du Laus n'a pas de vraie boucle aussi longue (23,5 km de pistes en cycles, le reste en tronçons sans alternative, dont la route du col d'Izoard) : 16,5 km à 30 % (15,8 km à 48 % avant) et 21,2 km à 35 % (17,7 km à 48 %), avec la note « aucune vraie boucle trouvée de 2 h 15 à 3 h 18 depuis Le Laus : les boucles proposées sont en partie en aller-retour sur le même chemin, seule option du réseau ici ; la plus longue vraie boucle trouvée fait 10,1 km, environ 1 h 57 ». Les 10 boucles à 25 % ou plus sont toutes dans ces cinq demandes, dites. Depuis Les Fonts, rien ne change (vraies boucles à 0–22 %). Jeu de test jurassien : que des vraies boucles avant comme après ; les « 3 à 17 % » affichés avant n'étaient que l'erreur de comptage par way.

## Temps du groupe (C11)

`suggest::suggest_for(&SiteData, &Request, Some(&ProfileModel))` choisit les boucles sur le temps du groupe du jour (`skimap_core::calibration`, a priori ou appris, [groupes.md](../../core/docs/groupes.md)) au lieu du temps du modèle : temps d'une boucle = médiane prédite pour le groupe (rythme par terrain, arrêts) ; écart au temps demandé, fenêtre de 75 à 110 %, retour prévu (× 1,2, marge de C7), heures limites et résumé parlé sur ce temps. Points de demi-tour : l'aller et le retour les plus rapides, en temps du modèle, convertis au temps du groupe sur le plat, dans 50 à 110 % du temps demandé (les coûts des routeurs portent des facteurs de style que le rythme du groupe multiplierait). `eta_min` reste le temps du modèle ; `group` (médiane, fourchette à 80 %, mouvement, arrêts) et la première raison disent le temps du groupe (« 3,7 km, D+ … m, environ <médiane> pour <groupe> (<10 %> à <90 %>, a priori ; modèle C5, skating : <temps du modèle>), retour prévu vers … »). Sans groupe (`suggest`, `suggest_for(…, None)`), rien ne change : sorties identiques octet pour octet aux sorties d'avant C11 sur les six demandes des deux sites (`suggestions_and_turnaround_without_group_golden`).

Avec des enfants dans le groupe (choix de prudence, [groupes.md](../../core/docs/groupes.md)) : le temps d'une boucle est le **côté lent** de sa fourchette (`ProfileModel::plan_quantile` : quantile à 80 %) au lieu de la médiane, pour la fenêtre, l'heure de retour (ce temps, sans le × 1,2 de C7 que le côté lent dépasse déjà) et le résumé parlé ; la limite C7 garde une heure de lumière (`GroupProfile::turnaround_params`) ; `GroupTime::plan_min` donne ce temps et la première raison dit « environ <médiane> … ; prévu sur <côté lent>, le côté lent : 8 chances sur 10 d'avoir fini ». Une boucle qui tient dans le temps demandé à la médiane mais pas sur le côté lent n'est proposée qu'après les autres, une au plus, marquée `if_all_goes_well` (« si tout va bien : … »). Famille « avec enfants débutants », 6 h, classique, jusqu'au rouge, 9 h : 13,9, 13,1 et 12,8 km puis 20,2 km « si tout va bien » sur le jeu de test jurassien (avant : 14,0, 13,1 et 12,9 km sur les anciens a priori et la médiane), 13,0 km sur le jeu de test alpin (avant : 7,0 et 10,7 km).

Famille (enfants de 6 et 9 ans, a priori), demandes de `presets` (jeu de test alpin 09/01/2026, jeu de test jurassien 22/02/2026, `suggestions_and_turnaround_for_a_family`) :

| Demande | Adulte : boucles (km max, modèle) | Famille : boucles (km max, modèle, temps du groupe : médiane et fourchette) |
|---|---|---|
| Jeu de test alpin, 2 h skating, retour avant 16 h | 1 (11,0 km, 111 min) | 2 (3,7 km, 34–35 min, 119–121 min ; 70 à 206) |
| Jeu de test alpin, 3 h classique | 1 (15,8 km, 180 min) | 2 (3,8 km, 40–45 min, 140–159 min ; 83 à 268) |
| Jeu de test alpin, 1 h 30 depuis Les Fonts | 1 (10,0 km, 91 min) | 0 (aucune boucle assez courte depuis Les Fonts) |
| Jeu de test jurassien, 2 h skating, retour avant 16 h | 3 (13,2 km, 115–119 min) | 3 (3,8 km, 28–32 min, 97–111 min) |
| Jeu de test jurassien, 3 h classique | 3 (19,4 km, 189–197 min) | 3 (5,7 km, 52–55 min, 179–188 min) |
| Jeu de test jurassien, 1 h 30, vertes et bleues | 3 (9,4 km, 69–91 min) | 2 (3,0 km, 27–28 min, 91–94 min) |

Voix : chaque boucle porte `voice`, `Event::LoopSummary` (« Boucle de treize kilomètres, environ deux heures, retour vers midi quinze. » : longueur au kilomètre, temps du modèle C5 sur la grille des durées, retour au quart d'heure le plus proche, dit comme on le dit), trois clips du pack.
