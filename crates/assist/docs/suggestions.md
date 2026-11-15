# A5 — suggestion d'itinéraire (#92)

[← skimap-assist](../README.md)

`suggest::suggest(&SiteData, &Request)` : « 2 h, skating, retour avant 16 h » → boucles classées. Sorties : `suggestions_<date>.json`, `.txt` et `.geojson` (WGS84, propriétés `rank`, `label`, `names`, `length_km`, `gain_m`, `eta_min`, `arrival`, `out_and_back_share`, `score`, `reasons`) pour le visualiseur.

Méthode, sur le graphe de C1 (`routes/piste_graph.json`, sens uniques respectés) et le temps de C5 (`EtaModel` : technique, température de l'heure de départ, classe de neige tirée de A3) :

1. départ rattaché au réseau (300 m au plus) ;
2. chaque nœud est un point de demi-tour possible si l'aller le plus rapide plus le retour le plus rapide font 50 à 110 % du temps demandé ; 40 au plus, répartis par échantillonnage du point le plus éloigné ;
3. retour le plus rapide avec les ways OSM de l'aller rendus 4 fois plus chers : une vraie boucle quand le réseau le permet, sinon un aller-retour, dit comme tel ;
4. niveau : les pistes plus difficiles que demandé coûtent 4 fois plus (`difficulty`, la couleur la plus facile des relations d'un way) ;
5. on garde les boucles de 75 à 110 % du temps demandé qui finissent (temps du modèle × 1,2, marge de C7) avant l'heure de retour demandée et l'heure limite de C7 ; les quasi-doublons (plus de 60 % d'arêtes communes) gardent la meilleure.

Points : écart au temps demandé (−2 × écart relatif), part d'aller-retour (−2 × part), pistes fermées (−10 × part) ou ouvertes (+1 × part) d'après les sources du jour (ENJ par la géométrie, à 30 m ; Nordic France par le nom), neige comme A1, terrain exposé aux avalanches (couche P16–P18 : −2 avec un BERA ≥ 3, −0,5 sans BERA ou BERA indisponible), météo des heures de la sortie comme A1, pistes plus difficiles que demandé (−2 × part), pistes nommées (+0,1 chacune, +0,5 au plus).

Limites du graphe, dites et non masquées : nœuds non atteignables depuis le départ (Cervières : 65 nœuds sur 189, 26,8 km, depuis Le Laus et 138 depuis Les Fonts, la « Liaison Le Laus - Les Fonts » et les pistes du Cancel s'arrêtant à 18 m l'une de l'autre ; Chapelle-des-Bois : 33 nœuds sur 649, 14,5 km, à cause des sens uniques), points de demi-tour sans retour possible.

Temps : 0,78 s pour trois demandes à Chapelle-des-Bois (649 nœuds : un itinéraire vers chaque nœud et retour, 40 précalculs de coûts), 0,1 s à Cervières.

Voix : chaque boucle porte `voice`, `Event::LoopSummary` (« Boucle de treize kilomètres, environ deux heures, retour vers midi et quart. » : longueur au kilomètre, temps du modèle C5 sur la grille des durées, retour au quart d'heure le plus proche, dit comme on le dit), trois clips du pack.
