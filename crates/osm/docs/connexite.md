# skimap-osm, P2 : connexité du réseau et corrections OSM à faire

[← skimap-osm](../README.md)

Mesures du 15/11/2026 (données OSM du 13/11/2026), avant → après la fermeture hors emprise :

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Graphe (nœuds / arêtes / composantes) | 189 / 223 / 5 → 189 / 224 / 4 | 623 / 741 / 13 → 649 / 773 / 11 |
| Composante principale | 124 nœuds, 41,4 km, fortement connexe | 533 nœuds, 250,6 km, 41 nœuds hors de la plus grande composante fortement connexe → 621 nœuds, 298,0 km (Risoux suisse rattaché), 35 nœuds hors |
| Requêtes aléatoires sans itinéraire (exemple `routes` de `core`, 2 000) | 0 → 0 | 146 (7,3 %) → 107 (5,35 %) |

Les 35 nœuds restants du jeu de test jurassien viennent de deux `piste:oneway=yes` à contresens dans OSM (étiquetés ainsi dès leur création, aucun way retourné depuis), à corriger dans OSM (`piste:oneway=-1`, ou retourner le way) :

- way **210382469** (boucle « Elodie Bourgeois Pin », relation 13769566, Chaux-Neuve) : il entre au nœud 1999008075 comme le way 189256510 ; les 10 autres ways à sens unique de la boucle la parcourent de 1999008075 vers 2204592756. 8 nœuds.
- way **471697226** (« Les Combettes », relations « La Combette » 6942987, « Patrice Bailly-Salins » 12142526, « Les Cernois Roz » 12206774) : il entre au nœud 2562904859 comme le way 471697224 et laisse le nœud 2562904873 sans arête entrante ; c'est le seul membre à contresens de « Patrice Bailly-Salins » et des « Cernois Roz ». 27 nœuds, dont toute la boucle du « Petit Canada ».

Simulation des deux corrections sur le graphe (oracle Python, 20 000 requêtes) : 4,80 % de requêtes sans itinéraire → 0,02 % ; il reste le nœud 2204592763, entrée à sens unique depuis la route (way 210382482, hors relation de piste).

**Le Laus (jeu de test alpin)** : le réseau des Fonts (« Liaison Le Laus - Les Fonts », relation 15582060) et celui du Laus et de l'Izoard ne se touchent pas dans OSM ; on n'ajoute pas de connecteur. La liaison (way 189296378, `highway=track`) finit au nœud 1999380464, posé sur la D 902 (way 40578401, route, pas piste) ; la piste du Laus (way 1037835272, `piste:type=nordic`, sans relation, qui rejoint « Le Cancel » au nœud 9559253950) commence au nœud 9559253951, relié à rien, à 4 m du nœud 167049553 de la D 902 et à 17,6 m du bout de la liaison. Même en suivant la route, aucun chemin OSM ne relie les deux. Le plan officiel (Nordic France, plan des pistes de la commune, mars 2024) montre la liaison qui arrive au départ des boucles du Laus, au parking. Correction OSM proposée : un way `piste:type=nordic` + `piste:grooming=classic+skating` du nœud 1999380464 au nœud 9559253957 du way 1037835272 (17,6 m, traversée de la D 902), ajouté à la relation 15582060.
