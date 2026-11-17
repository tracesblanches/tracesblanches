# skimap-osm, P2 : pistes nordiques d'un site

[← skimap-osm](../README.md)

Source : extrait quotidien d'OpenSnowMap `planet_pistes.osm.gz` (≈ 200 Mo, toutes les pistes de la planète, ODbL), téléchargé par le téléchargeur commun (source `opensnowmap`, 1 requête/min, cache 1 jour). La date des données OSM vient de `planet_pistes-state.txt`. On n'interroge pas Overpass.

Lecture : `parse::read_file` lit le fichier en flux (décompression sur un fil à part) et ne garde que la France métropolitaine élargie (`extract::france_aoi`). Mesure sur le fichier entier (1,8 Go de XML, 8,7 M nœuds, 483 k ways, 17 k relations) : 3,5 s, dont la classification.

Sélection (`pistes::classify`) :

- way dont `piste:type` contient `nordic`, `hike` ou `snowshoe`, ou way membre d'une relation de piste de ces types (ou `route=ski`) : beaucoup de tronçons de route ne portent aucune étiquette de piste, seule la relation dit qu'ils sont damés ;
- `area=yes` exclu ; un site garde les ways ayant au moins un sommet dans son emprise, plus les ways hors emprise qui ramènent le réseau dans l'emprise (ci-dessous, `in_box = false` ; mesures dans [connexite.md](connexite.md)).

Réseau refermé hors de l'emprise (`extract::site_pistes`, `Graph::reentry_paths`) : couper le réseau à l'emprise transforme une piste à sens unique qui sort de l'emprise et y revient en un cul-de-sac dont on ne repart pas (jeu de test jurassien : « Voie du tram », « Le Gy Large », « Le Chocolat » à l'ouest de 6,02° E). Le réseau complet autour du site (pistes à 10 km au plus des pistes du site) est construit ; depuis chaque nœud du site relié à l'extérieur, un Dijkstra sur les seules arêtes extérieures (sens uniques respectés, 10 km au plus) cherche les nœuds du site qu'il atteint ; chaque nœud que le réseau du site (avec les chemins déjà gardés) n'atteint pas donne un chemin à garder. Jeu de test jurassien : 6 chemins, 32 ways, 9,2 km (le plus long 2,9 km) ; Jeu de test alpin : 1 way de 1,5 km (« Souliers », qui relie deux morceaux du site). Résultat identique de 3 à 30 km de rayon. Les chiffres de ways, de km et de relations de `pistes.meta.json` ne comptent que les ways de l'emprise (ceux des rapports) ; `ways_outside_box` et `km_outside_box` comptent les autres ; la liste des chemins est dans `stats.reentries`.

Sens unique : seul `piste:oneway` du way compte. `oneway=*` est le sens des voitures (aucun way de piste du jeu de test jurassien n'a `oneway=yes`). Les relations ne donnent pas de sens : « Fabrice Guy » (relation 1461804, `oneway=yes`) parcourt ses ways à l'aller puis au retour (membres 0 à 16 puis 53 à 68, les mêmes ways en sens inverse) ; les rôles `forward` / `backward` décrivent le parcours d'une relation, pas une interdiction (way 210262719 : `piste:oneway=no` et rôle `backward` dans « Le Verdet »). Les contradictions sont signalées, jamais corrigées : `oneway_sinks` / `oneway_sources` de `pistes.meta.json` listent les nœuds (degré ≥ 2) dont toutes les arêtes sont à sens unique et entrent toutes (ou sortent toutes).

Attributs : nom et couleur viennent des relations, car la difficulté des ways est incohérente (sous « Greffier », noire, les ways disent `easy`, `intermediate`, `advanced` ou `novice`). La difficulté d'un tronçon est celle de la couleur de ses relations (verte = `novice`, bleue = `easy`, rouge = `intermediate`, noire = `advanced`), sinon leur `piste:difficulty`, sinon celle du way. Un tronçon partagé prend la difficulté **la plus facile** (`difficulty`, une boucle verte est skiable en entier au niveau vert) et garde la plus dure (`difficulty_max`). Damage (`piste:grooming`, du way sinon de la relation), sens unique (`piste:oneway`, géométrie retournée pour `-1`), éclairage.

Sorties dans `data/out/<site>/pistes/` (WGS84) :

| Fichier | Contenu |
|---|---|
| `pistes.geojson` | un LineString par way, propriétés documentées dans `pistes.meta.json` |
| `graph_edges.geojson` | graphe de routage : ways coupés aux jonctions (nœud partagé, extrémité), plus des connecteurs (`kind = connector`) fermant les trous de 10 m au plus entre un cul-de-sac et une autre piste |
| `graph_nodes.geojson` | nœuds du graphe (identifiant OSM, degré, composante) |
| `pistes.meta.json` | sources (URL, licence, attribution, ETag, sha256, date), date de validité, propriétés, paramètres, chiffres clés |

API : `extract::build(site, &layout, &downloader)` ; `extract::load_planet` puis `extract::build_from` pour lire l'extrait une seule fois pour plusieurs sites. Exemple : `cargo run --release -p skimap-osm --example pistes`.

Chiffres au 13/11/2026 (données OSM) comparés aux rapports :

| | Ce crate | Rapport |
|---|---|---|
| Jeu de test jurassien : pistes testées présentes | 13/13, union 151 ways, 65,7 km | 02 : 13/13, 65,7 km |
| Jeu de test jurassien : relations | 84 nordic + 3 autres | 02 : 84 + 3 |
| Jeu de test jurassien : ways étiquetés nordic | 599 ways, 270,5 km | 02 : 598 ways, ≈ 270 km |
| Jeu de test alpin : `piste:type=nordic` exact, emprise entière | 102 ways, 24,9 km | 04 : « environ 25 km » (102 ways, 24,9 km) |
| Jeu de test alpin : nordique avec membres des relations, latitude moyenne ≥ 44,82 | 34,5 km | 07 : 34,5 km |
| Jeu de test alpin : idem avec `hike` | 37,7 km (114 ways), dont 3,2 km `hike` | 02 : 37,7 km (115 ways), dont 3,2 km |

Les trois chiffres du jeu de test alpin ne se contredisent pas : 25 km ne compte que les ways étiquetés exactement `nordic` (la requête Overpass du rapport 04 excluait `nordic;hike` et les tronçons de route qui ne sont pistes que par leur relation, comme la route de l'Izoard) sur toute l'emprise, Arvieux compris ; 34,5 km ajoute les membres des relations mais ne garde que le versant nord de l'Izoard ; 37,7 km ajoute les 3,2 km de `hike`. L'emprise entière du site (Brunissard et Arvieux compris) contient 66,7 km de pistes, dont 61,6 km nordiques.
