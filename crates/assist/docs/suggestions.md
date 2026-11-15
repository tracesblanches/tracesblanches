# A5 — suggestion d'itinéraire (#92)

[← skimap-assist](../README.md)

`suggest::suggest(&SiteData, &Request)` : « 2 h, skating, retour avant 16 h » → boucles classées. Sorties : `suggestions_<date>.json`, `.txt` et `.geojson` (WGS84, propriétés `rank`, `label`, `names`, `length_km`, `gain_m`, `eta_min`, `arrival`, `out_and_back_share`, `score`, `reasons`) pour le visualiseur.

Méthode, sur le graphe de C1 (`routes/piste_graph.json`, sens uniques respectés) et le temps de C5 (`EtaModel` : technique, température de l'heure de départ, classe de neige tirée de A3) :

1. départ rattaché au réseau (300 m au plus) ;
2. chaque nœud est un point de demi-tour possible si l'aller le plus rapide plus le retour le plus rapide font 50 à 110 % du temps demandé ; 40 au plus, répartis par échantillonnage du point le plus éloigné ;
3. retour le plus rapide avec les ways OSM de l'aller rendus 4 fois plus chers : une vraie boucle quand le réseau le permet, sinon un aller-retour, dit comme tel ;
4. niveau : les pistes plus difficiles que demandé coûtent 4 fois plus (`difficulty`, la couleur la plus facile des relations d'un way) ;
5. on garde les boucles de 75 à 110 % du temps demandé qui finissent (temps du modèle × 1,2, marge de C7) avant l'heure de retour demandée et l'heure limite de C7 ; les quasi-doublons (plus de 60 % d'arêtes communes) gardent la meilleure.

Points : écart au temps demandé (−2 × écart relatif), part d'aller-retour (−2 × part), pistes fermées (−10 × part) ou ouvertes (+1 × part) d'après les sources du jour (ENJ par la géométrie, à 30 m ; Nordic France par les voies OSM que la couche `status` lui rattache, `osm_way_ids` de `status.geojson`), neige comme A1, terrain exposé aux avalanches (couche P16–P18 : −2 avec un BERA ≥ 3, −0,5 sans BERA ou BERA indisponible), météo des heures de la sortie comme A1, pistes plus difficiles que demandé (−2 × part), pistes nommées (+0,1 chacune, +0,5 au plus).

Limites du graphe, dites et non masquées : nœuds non atteignables depuis le départ, points de demi-tour sans retour possible. Au 15/11/2026 (graphe P2 refermé hors de l'emprise) : Cervières 65 nœuds sur 189 (26,8 km) depuis Le Laus et 138 depuis Les Fonts, inchangé, la « Liaison Le Laus - Les Fonts » s'arrêtant sur la D 902 à 17,6 m de la piste du Laus (trou des données OSM, `crates/osm/docs/connexite.md`) ; Chapelle-des-Bois 33 sur 649 (14,5 km) depuis le village, contre 96 sur 623 avant la fermeture du réseau hors de l'emprise. Avec les corrections OSM proposées (simulées sur le graphe) : Cervières 14 sur 189 depuis chacun des deux départs ; Chapelle 29 sur 649 (28 dans des réseaux séparés, plus l'entrée à sens unique 2204592763).

Temps : 0,78 s pour trois demandes à Chapelle-des-Bois (649 nœuds : un itinéraire vers chaque nœud et retour, 40 précalculs de coûts), 0,1 s à Cervières.

## Temps du groupe (C11)

`suggest::suggest_for(&SiteData, &Request, Some(&ProfileModel))` choisit les boucles sur le temps du groupe du jour (`skimap_core::calibration`, a priori ou appris, [groupes.md](../../core/docs/groupes.md)) au lieu du temps du modèle : temps d'une boucle = médiane prédite pour le groupe (rythme par terrain, arrêts) ; écart au temps demandé, fenêtre de 75 à 110 %, retour prévu (× 1,2, marge de C7), heures limites et résumé parlé sur ce temps. Points de demi-tour : l'aller et le retour les plus rapides, en temps du modèle, convertis au temps du groupe sur le plat, dans 50 à 110 % du temps demandé (les coûts des routeurs portent des facteurs de style que le rythme du groupe multiplierait). `eta_min` reste le temps du modèle ; `group` (médiane, fourchette à 80 %, mouvement, arrêts) et la première raison disent le temps du groupe (« 3,7 km, D+ … m, environ <médiane> pour <groupe> (<10 %> à <90 %>, a priori ; modèle C5, skating : <temps du modèle>), retour prévu vers … »). Sans groupe (`suggest`, `suggest_for(…, None)`), rien ne change : sorties identiques octet pour octet aux sorties d'avant C11 sur les six demandes des deux sites (`suggestions_and_turnaround_without_group_golden`).

Avec des enfants dans le groupe (choix de prudence, [groupes.md](../../core/docs/groupes.md)) : le temps d'une boucle est le **côté lent** de sa fourchette (`ProfileModel::plan_quantile` : quantile à 80 %) au lieu de la médiane, pour la fenêtre, l'heure de retour (ce temps, sans le × 1,2 de C7 que le côté lent dépasse déjà) et le résumé parlé ; la limite C7 garde une heure de lumière (`GroupProfile::turnaround_params`) ; `GroupTime::plan_min` donne ce temps et la première raison dit « environ <médiane> … ; prévu sur <côté lent>, le côté lent : 8 chances sur 10 d'avoir fini ». Une boucle qui tient dans le temps demandé à la médiane mais pas sur le côté lent n'est proposée qu'après les autres, une au plus, marquée `if_all_goes_well` (« si tout va bien : … »). Famille « avec enfants débutants », 6 h, classique, jusqu'au rouge, 9 h : 13,9, 13,1 et 12,8 km puis 20,2 km « si tout va bien » à Chapelle-des-Bois (avant : 14,0, 13,1 et 12,9 km sur les anciens a priori et la médiane), 13,0 km à Cervières (avant : 7,0 et 10,7 km).

Famille (enfants de 6 et 9 ans, a priori), demandes de `presets` (Cervières 09/01/2026, Chapelle-des-Bois 22/02/2026, `suggestions_and_turnaround_for_a_family`) :

| Demande | Adulte : boucles (km max, modèle) | Famille : boucles (km max, modèle, temps du groupe : médiane et fourchette) |
|---|---|---|
| Cervières, 2 h skating, retour avant 16 h | 1 (11,0 km, 111 min) | 2 (3,7 km, 34–35 min, 119–121 min ; 70 à 206) |
| Cervières, 3 h classique | 1 (15,8 km, 180 min) | 2 (3,8 km, 40–45 min, 140–159 min ; 83 à 268) |
| Cervières, 1 h 30 depuis Les Fonts | 1 (10,0 km, 91 min) | 0 (aucune boucle assez courte depuis Les Fonts) |
| Chapelle, 2 h skating, retour avant 16 h | 3 (13,2 km, 115–119 min) | 3 (3,8 km, 28–32 min, 97–111 min) |
| Chapelle, 3 h classique | 3 (19,4 km, 189–197 min) | 3 (5,7 km, 52–55 min, 179–188 min) |
| Chapelle, 1 h 30, vertes et bleues | 3 (9,4 km, 69–91 min) | 2 (3,0 km, 27–28 min, 91–94 min) |

Voix : chaque boucle porte `voice`, `Event::LoopSummary` (« Boucle de treize kilomètres, environ deux heures, retour vers midi quinze. » : longueur au kilomètre, temps du modèle C5 sur la grille des durées, retour au quart d'heure le plus proche, dit comme on le dit), trois clips du pack.
