# skimap-srn, G5 : domaines de jeu SRN hors stations

[← skimap-srn](../README.md)

## Méthode

1. **Itinéraires**, un seul modèle (`route::Route`) pour quatre fournisseurs :
   - **OSM** (extrait OpenSnowMap, déjà lu par P2/P3) : `piste:type=nordic` + `piste:grooming=backcountry` = SRN ; `skitour` = ski de randonnée ; `hike` / `snowshoe` = raquettes. Une relation `route=piste|ski` est un itinéraire quand son propre `piste:type` et son damage donnent une classe (une relation `nordic` sans `backcountry` est une piste damée, même si quelques voies sont `backcountry`), ou quand la majorité de sa longueur est d'une classe ; les autres voies classées forment des réseaux de voies connexes.
   - **Geotrek** : la liste officielle (`utilisateurs.geotrek.fr/clients_map.geojson`, 81 instances) donne le site de chaque instance ; l'API est lue dans la page (`"apiUrl":"…"`, Geotrek-rando v3). Les racines trouvées le 15/11/2026 (61 instances) et la raison de l'absence pour les 20 autres sont gardées dans `data/geotrek_apis.json` : les sites morts (Tarn, Ardèche Hermitage : délai dépassé ; Volcans d'Auvergne : certificat auto-signé) ne sont pas relus à chaque construction, le téléchargeur attendrait chaque délai ; une instance nouvelle est découverte à la volée. Seules les instances dont l'emprise passe à moins de 0,2° (≈ 20 km) d'un domaine damé du recensement sont lues (la neige) ; une API servie par plusieurs instances est lue une fois, ses itinéraires sont attribués à toutes. Une pratique nommée « nordique » ou « SRN » donne des itinéraires SRN ; « marche nordique », « ski de fond », « snow trail » sont écartés ; les pratiques « raquettes » sont seulement comptées.
   - **Camptocamp** (France, `a=14274`) : `act=skitouring&trat=1.1,1.3` et les recherches `q=nordique`, `q=ski de fond`. Activité `skitouring`, titre « nordique » (hors « site / domaine / centre / foyer / espace nordique », noms de lieux) ou « ski de fond » et cotation ≤ 2.1 : SRN ; cotation 1.1 à 1.3 avec au plus 500 m de dénivelé : ski de randonnée facile (pentes de moins de 30°, compatible SRN) ; plus de dénivelé : ski de randonnée. Seuls les documents avec un tracé détaillé (`geom_detail`) donnent un itinéraire ; les autres deviennent des points à compléter.
   - **DATAtourisme** (`datatourisme-tour.csv` du jour, data.gouv.fr) : un tour dont le nom dit « nordique » (sauf marche nordique, ski de fond, domaine ou site nordique) ou « ski de randonnée » est lu sur `data.datatourisme.fr` (`?format=jsonld`) ; il faut le thème `SkiTouring` (« randonnée nordique » seul est de la marche nordique en Dordogne, de la marche dans les Vosges) ; le GPX vient de la fiche (`hasRepresentation` → `ebucore:hasRelatedResource` → `ebucore:locator`, type `GPX`). Les tours raquettes sont seulement comptés.
2. **Hors stations** : chaque itinéraire est densifié à 100 m en Lambert-93 ; sa part de points à moins de 30 m d'une piste nordique damée (OSM, classement P2 : `nordic` sans `backcountry`) est son `groomed_share`. Un itinéraire (amorce ou rattaché) à 60 % ou plus sur des pistes damées est écarté (c'est l'affaire de la station), sauf s'il est `backcountry` dans OSM. Mesuré : GTJ à ski de fond de Camptocamp 0,72 (écartée), GTJ à ski de randonnée nordique 0,47 (gardée). Les enveloppes des domaines (P3) contiennent les forêts entre les pistes, où se pratique le SRN : une première version qui écartait à 75 % dans ces enveloppes perdait la GTJ SRN ; elles ne servent plus qu'à lister les stations recoupées.
3. **Zones** (`areas`) :
   - **traversée** : un itinéraire amorce (SRN ou ski de randonnée facile) point à point d'au moins 25 km ; emprise = couloir de 500 m ;
   - **domaine de jeu** : les itinéraires amorces de moins de 60 km qui passent à moins de 1,5 km l'un de l'autre, d'au moins 2 km au total ; emprise = fermeture morphologique (tampon de 750 m puis −250 m : les écarts de moins de 1,5 km entre itinéraires sont comblés, pas les grands versants vides entre deux branches, que l'enveloppe concave remplissait), réunie au couloir de 500 m (le tampon négatif d'i_overlay laissait jusqu'à 15 % des points d'un itinéraire dehors), sans lambeaux de moins de 1 ha ni trous de moins de 1 km². Un groupe réduit à une traversée déjà gardée n'est pas un domaine. Une traversée peut donc appartenir à un domaine (les traversées des Hauts-Plateaux) ; une traversée de plus de 60 km (GTJ) ne colle pas les domaines entre eux ;
   - **rattachés** : un itinéraire raquettes ou ski de randonnée va à la zone qui contient au moins la moitié de ses points (domaine d'abord) ; il n'agrandit pas l'emprise.
4. **France** : commune (geo.api.gouv.fr) du point représentatif (point d'itinéraire le plus proche du barycentre), sinon d'un des points à 0, 25, 50, 75, 100 % du plus long itinéraire (la GTJ longe la frontière : son barycentre est en Suisse) ; sans commune française, la zone est écartée. Massif : groupe du département (groupes du rapport 08, plus « Ardennes et Morvan »).
5. **Stations** : chaque zone liste les domaines damés recoupés (`stations` : part de l'emprise, km d'itinéraires dedans) et les autres zones recoupées (`crosses`).
6. **Noms** : traversée = nom de l'itinéraire ; domaine = nom tenu à la main (`data/names.json`, 3 noms, chacun avec sa source) donné à la seule zone dont le point représentatif est le plus proche de son point, dans son rayon ; sinon nom de l'itinéraire s'il est seul, sinon commune.

## Sources et licences

| Source | Licence lue | Conséquence |
|---|---|---|
| OSM, extrait OpenSnowMap | ODbL 1.0 | la couche est une base dérivée : attribution, partage à l'identique |
| Geotrek, PNR du Vercors (`geo.pnr-vercors.fr/api/v2`, pratique « Nordique ») | aucune licence dans l'API ni dans les deux pages « Mentions légales » (lues le 15/11/2026) ; le même PNR publie le réseau d'itinéraires de cette base Geotrek sous Licence Ouverte 2.0 sur data.gouv.fr (« Itinéraires de randonnée sur le Parc du Vercors », couche `itineraires.gpkg` lue : tracés d'été, pas d'itinéraire nordique) | utilisée, marquée « à confirmer » dans chaque itinéraire et dans `srn.meta.json` ; un courriel au PNR lèverait le doute |
| Geotrek, autres instances | aucune licence vue ; seules des pratiques raquettes (CD05, Écrins, AURA Tourisme, CD04, Livradois-Forez, Gavarnie, CD39 = copie ENJ « tous droits réservés ») | comptées, pas téléchargées |
| Camptocamp | CC BY-SA 3.0, champ `license` de chaque document ; auteurs sur la page d'historique | attribution par itinéraire (`attribution`) |
| DATAtourisme | Licence Ouverte 2.0 : « L'utilisation des données mises à disposition via le dispositif DATAtourisme est soumise aux termes de cette licence » ; paternité = `hasBeenCreatedBy`, date de mise à jour (datatourisme.fr, article du 21/08/2019 mis à jour le 22/12/2025) | GPX référencé par la fiche (`ebucore:locator`), hébergé chez Tourinsoft : utilisé sous la licence de la fiche, attribution « producteur via DATAtourisme, mise à jour du … » |
| geo.api.gouv.fr | Licence Ouverte 2.0 | commune et département |

Sources examinées et écartées :

- **ENJ (Geotrek Snowmap)** : « GTJ ski » (23 étapes damées) et « GTJ raquettes », pas de GTJ SRN ; « dispose de tous les droits » (rapport 08).
- **Camptocamp 780666 « Grande Traversée du Jura »** : la GTJ à ski de fond sur pistes damées (« en suivant les pistes de ski nordique balisées et damées ») : 72 % de son tracé à moins de 30 m d'une piste damée, écartée.
- **gtj.asso.fr** : page Cloudflare (403, rapport 04) ; **skirandonneenordique.com** : aucune licence (rapport 04) ; **Wikiloc**, **Visorando** : 403 ou sans ski (rapport 04).
- **Overpass** : aucune relation de traversée SRN (« GTJ », « GTV », « Hauts-Plateaux ») hors de l'extrait ; la GTV d'OSM est la GTV VTT du Vaucluse.

## Résultats (15/11/2026)

`cargo run --release -p skimap-srn --example srn` : **43 zones** en France, **40 domaines de jeu et 3 traversées** ; 29 avec un itinéraire dit SRN, 14 seulement « compatibles » (ski de randonnée facile Camptocamp) ; **87 itinéraires amorces distincts, 565 km** (643 km en comptant deux fois les traversées des Hauts-Plateaux, qui sont aussi dans le domaine) et 120 km rattachés (raquettes, ski de randonnée) ; 30 zones recoupent un domaine damé ; 5 zones écartées hors de France.

| Massif | Domaines | Traversées | km d'itinéraires distincts |
|---|---|---|---|
| Alpes du Nord | 17 | 2 (Hauts-Plateaux du Vercors) | 295,7 |
| Jura | 5 | 1 (GTJ à ski de randonnée nordique) | 135,6 |
| Alpes du Sud | 8 | 0 | 80,8 |
| Pyrénées | 4 | 0 | 23,6 |
| Massif central | 3 | 0 | 13,6 |
| Vosges | 2 | 0 | 11,9 |
| Ardennes et Morvan | 1 | 0 | 4,2 |

Les plus grandes : Les Moises – Hermones (Chablais, 87 km surtout OSM `backcountry`, 52 km², recoupe Les Moises, Hirmentaz, Les Brasses, Col du Feu), Hauts-Plateaux du Vercors (78 km, 47 km²), La Molière (28 km), Pionniers-Ambel (23 km), Thorame-Haute / Colle-Saint-Michel (20 km), Chamrousse (14 km).

Par fournisseur (itinéraires dans les zones) : OSM 55 SRN (182 km), 35 raquettes (76 km), 10 ski de randonnée (13 km) ; Geotrek 9 SRN (165 km, PNR du Vercors) ; Camptocamp 22 ski de randonnée facile (118 km), 5 ski de randonnée (30 km) ; DATAtourisme 1 SRN (la GTJ, 100 km). Lus en tout : OSM 115 SRN (241 km), 1 086 raquettes, 659 ski de randonnée ; Camptocamp 309 documents listés (78 faciles, 225 plus raides, 3 SRN, 3 écartés), 104 avec tracé gardés ; DATAtourisme 27 595 tours, 10 candidats lus, 6 rejetés par leurs thèmes (marche nordique, marche, ski-joëring). Écartés sur pistes damées : 133 raquettes OSM, 1 ski de randonnée OSM, 2 Camptocamp (GTJ à ski de fond 0,72, Izoard versant S 0,96).

Geotrek : 48 instances près d'un domaine damé, 38 avec une API lue, 10 sans API (pas de site, page sans `apiUrl`, site injoignable) ; une seule pratique SRN (« Nordique », PNR du Vercors, 9 itinéraires) ; pratiques raquettes comptées : ENJ 232, CD39 94 (copie de l'ENJ), CD05 50, Écrins 45, AURA 26 (dont les itinéraires nordiques du Vercors, republiés en « Raquettes »), CD04 13, Livradois-Forez 12, Gavarnie 4.

Temps : premier passage 267 s (Camptocamp 107 s pour une centaine de documents à 1 requête par seconde, communes 137 s), dont la découverte Geotrek par les sites, remplacée depuis par la table ; ensuite 6 s plus la lecture de l'extrait (3 à 21 s selon la charge du poste), tout étant en cache 30 jours.

## Vérifications à la main

Rendus sur fond Plan IGN v2 (les tuiles d'openstreetmap.org refusent le client générique, 403) : tracés, emprise, domaines damés.

| Zone | Tracé | Longueur calculée / publiée | Constat |
|---|---|---|---|
| GTJ à ski de randonnée nordique (traversée) | GPX Tourinsoft de la fiche DATAtourisme `ITIBFC000V51PJZ0` | 99,8 km / 100 km (`tourDistance` ; jura-tourism : « le circuit long de 100 km » ; montagnes-du-jura : 130 km dans le texte, 100 km dans l'encadré, rapport 04) ; D+ publié 1 664 m | départ Mouthe (46,7068 N, 6,2093 E), arrivée Giron (46,2262 N, 5,7751 E) par Chapelle-des-Bois, Bellefontaine, le Risoux, Bois-d'Amont, Les Rousses, Lajoux, La Pesse ; 47 % à moins de 30 m d'une piste damée, recoupe 9 domaines damés (Lajoux 16 % de l'emprise, La Pesse 16 %, Les Rousses 8 %, Le Risoux 6 %) |
| GTV : traversée nordique des Hauts-Plateaux, Corrençon – Col de Rousset | Geotrek PNR du Vercors 29411 | 36,38 km / 36,41 km (`length_2d`), D+ 1 263 m | de Corrençon-en-Vercors au Col du Rousset (Saint-Agnan-en-Vercors) par la Réserve naturelle ; le PNR la présente comme « "LA" Traversée mythique des hauts plateaux du Vercors » ; ses GTV (« Grandes Traversées du Vercors ») sont à pied et à VTT : aucune « GTV à ski » distincte dans ses données |
| Hauts-Plateaux du Vercors (domaine) | 29411 et 29403 (Combeau – Corrençon, 41,69 / 41,72 km, de Châtillon-en-Diois à Corrençon) | 78,1 km, 46,6 km² | les deux traversées partent de Corrençon, se séparent au sud du Grand Veymont vers le Col de Rousset et le vallon de Combeau ; les deux traversées sont dans `crosses` ; le Raid nordique Camptocamp 1648513 (sans tracé) tombe dans l'emprise |
| Les Moises – Hermones | 13 relations et 1 réseau OSM `backcountry`, 1 Camptocamp facile (Pointe de Miribel par Plaine-Joux) | 87,1 km, 51,6 km² | Tour des Hermones, Tête du Char, Pointe de Miribel, Très le Mont, Col du Feu, Plaine-Joux : vrais itinéraires non damés de la Vallée Verte, membres du site « Domaine nordique des Moises » |
| La Molière | Geotrek 29218 + Camptocamp 51389 | 28,5 km (17,7 publiés pour l'aller-retour Geotrek) | crête de la Molière depuis le col de la Croix Perrin, montée Camptocamp depuis Lans les Aigaux |
| Pionniers-Ambel | Geotrek 29401 | 22,66 / 22,67 km | boucle du plateau d'Ambel depuis la route des Pionniers |
| Château-Ville-Vieille | OSM « Traversée du Queyras - Chanteloube » (rapport 04 : 9,3 km) | 10,0 km | Aiguilles – Molines par la forêt de Chanteloube ; recoupe Queyras - Aiguilles (58 % de l'emprise) |
| Judenhut (Vosges) | OSM `backcountry` | 3,9 km | boucle sous le Grand Ballon, dans l'enveloppe de Markstein-Grand Ballon (53 %) |
| Thorame-Haute | OSM « Champlatte », « Champlatte bis » | 19,8 km | Colle-Saint-Michel ; tout dans l'enveloppe du domaine damé (P3 compte ces voies `backcountry` comme nordiques) |

Contrôles automatiques sur la couche produite (`cargo test -p skimap-srn --test real_layer -- --ignored`, 3 tests) : GTJ de 95 à 110 km et publiée 100 km, Corrençon – Col de Rousset à moins de 3 % de sa longueur publiée, domaine des Hauts-Plateaux avec ses deux traversées ; chaque zone française, d'un massif, avec licences et itinéraires présents dans `srn_routes.geojson` ; chaque point de chaque itinéraire amorce dans l'emprise de sa zone (0 point dehors).

## Limites et suites

- **Ce qui manque** (aucun tracé sous licence ouverte trouvé) : la plupart des plateaux cités par le ticket. Chartreuse (Hauts de Chartreuse), crêtes des Vosges (Hohneck, Gaschney : deux fiches DATAtourisme « ski de randonnée » sans GPX), Aubrac, Cézallier, Cantal, Mont Lozère (deux itinéraires Camptocamp SRN sans tracé : « Parcours randonnée nordique » du Finiels, Pic Cassini « à ski de fond »), plateau des Glières, Queyras hors Chanteloube, Haut-Jura hors GTJ (Risoux : APPB, itinéraires balisés seulement). `srn_hints.geojson` liste 205 documents sans tracé, dont 196 hors de toute zone (2 SRN, 48 ski de randonnée facile, 146 ski de randonnée) : la liste à compléter.
- **GTV** : pas de « Grande Traversée du Vercors à ski » distincte dans les sources ; la traversée nordique des Hauts-Plateaux du PNR en tient lieu (à confirmer auprès du PNR).
- **Licences à confirmer** : itinéraires nordiques du PNR du Vercors (aucune licence affichée, voir plus haut) ; GPX de la GTJ hébergé chez Tourinsoft, utilisé sous la Licence Ouverte de la fiche DATAtourisme qui le référence.
- **Instances Geotrek non lues** : Volcans d'Auvergne (certificat auto-signé), Lozère, Hautes-Pyrénées, Ariège (pas de site dans la liste), Monts d'Ardèche, Alpes-Maritimes, Serre-Ponçon (page sans `apiUrl`, sans doute Geotrek-rando v2), CD64 (403, rapport 08).
- **Raquettes Geotrek et DATAtourisme** : seulement comptées (licences non affichées ; centaines de requêtes pour des itinéraires seulement rattachés).
- **Seuils** : 1,5 km, 2 km, 25 km, 60 km, 30 m et 60 % sont des règles de réseau, pas de terrain ; « compatible » repose sur la cotation Camptocamp (≤ 1.3) et 500 m de dénivelé, sans pente calculée.
- **Noms** : 3 noms tenus à la main ; les autres domaines portent le nom de leur itinéraire ou de la commune du point représentatif.
- **Zones propices calculées** (pente faible, forêt praticable, enneigement, accès) : non faites. Il faudrait le relief (P5) et la neige (P6) de toutes les zones de montagne de France, aujourd'hui calculés pour les deux sites seulement ; suite : lancer P5 et P6 sur l'emprise de chaque zone (G2) puis classer pente < 20°, forêt claire (BD Forêt V2), neige, accès (routes déneigées).
- Pour en faire des sites (G2) : chaque zone a son emprise, sa commune, ses itinéraires, ses stations recoupées ; les règles des zones protégées (Réserve des Hauts-Plateaux, APPB du Haut-Jura) viendront de la couche `zones` par site.
