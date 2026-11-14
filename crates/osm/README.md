# skimap-osm

Extraction des pistes nordiques OSM et recensement des domaines de fond (pipeline). Tickets : P2 (#15), P3 (#16).

## P2 : pistes nordiques d'un site

Source : extrait quotidien d'OpenSnowMap `planet_pistes.osm.gz` (≈ 200 Mo, toutes les pistes de la planète, ODbL), téléchargé par le téléchargeur commun (source `opensnowmap`, 1 requête/min, cache 1 jour). La date des données OSM vient de `planet_pistes-state.txt`. On n'interroge pas Overpass.

Lecture : `parse::read_file` lit le fichier en flux (décompression sur un fil à part) et ne garde que la France métropolitaine élargie (`extract::france_aoi`). Mesure sur le fichier entier (1,8 Go de XML, 8,7 M nœuds, 483 k ways, 17 k relations) : 3,5 s, dont la classification.

Sélection (`pistes::classify`) :

- way dont `piste:type` contient `nordic`, `hike` ou `snowshoe`, ou way membre d'une relation de piste de ces types (ou `route=ski`) : beaucoup de tronçons de route ne portent aucune étiquette de piste, seule la relation dit qu'ils sont damés ;
- `area=yes` exclu ; un site garde les ways ayant au moins un sommet dans son emprise.

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
| Chapelle-des-Bois : pistes testées présentes | 13/13, union 151 ways, 65,7 km | 02 : 13/13, 65,7 km |
| Chapelle-des-Bois : relations | 84 nordic + 3 autres | 02 : 84 + 3 |
| Chapelle-des-Bois : ways étiquetés nordic | 599 ways, 270,5 km | 02 : 598 ways, ≈ 270 km |
| Cervières : `piste:type=nordic` exact, emprise entière | 102 ways, 24,9 km | 04 : « environ 25 km » (102 ways, 24,9 km) |
| Cervières : nordique avec membres des relations, latitude moyenne ≥ 44,82 | 34,5 km | 07 : 34,5 km |
| Cervières : idem avec `hike` | 37,7 km (114 ways), dont 3,2 km `hike` | 02 : 37,7 km (115 ways), dont 3,2 km |

Les trois chiffres de Cervières ne se contredisent pas : 25 km ne compte que les ways étiquetés exactement `nordic` (la requête Overpass du rapport 04 excluait `nordic;hike` et les tronçons de route qui ne sont pistes que par leur relation, comme la route de l'Izoard) sur toute l'emprise, Arvieux compris ; 34,5 km ajoute les membres des relations mais ne garde que le versant nord de l'Izoard ; 37,7 km ajoute les 3,2 km de `hike`. L'emprise entière du site (Brunissard et Arvieux compris) contient 66,7 km de pistes, dont 61,6 km nordiques.

## P3 : domaines de fond, France

Regroupement maison (`domains::cluster`), lancé sur toutes les pistes de la zone France de l'extrait (≈ 33 000 ways, 0,9 s) :

1. **Réseau** : graphe des pistes (nordic, hike, snowshoe) coupé aux jonctions ; deux réseaux à moins de 150 m sont reliés (OpenSkiMap : 500 m de proche en proche).
2. **Relations `site=piste`** : chaque relation site avec du nordique est une graine ; un way sert la relation la plus précise qui le contient (« Le Risoux » dans « Les Rousses » : une relation dont les ways sont un sous-ensemble strict d'une autre, ou qui en est membre, est un sous-site). Deux relations site ne fusionnent jamais.
3. **Cœurs de réseau** : un pont (arête dont le retrait coupe le réseau) est une liaison quand chaque côté pèse au moins 5 km ; sans ces ponts, les morceaux d'au moins 5 km sont des cœurs. Un cœur sans relation site devient une graine.
4. **Partage** : Dijkstra multi-sources sur le réseau, chaque nœud va à la graine la plus proche.
5. **Fusion** : deux cellules voisines (pas deux relations site) fusionnent si la liaison qui les relie fait moins de 6 km (Le Laus – Les Fonts à Cervières) ; une liaison plus longue est un passage entre vallées (Cervières – Brunissard par l'Izoard : 13,2 km).
6. **Cohérence des relations** : une relation de piste de 25 km au plus coupée entre deux domaines va entière au domaine qui en a le plus ; la coupure de l'Izoard tombe ainsi au col, entre la piste « Izoard » de Cervières et la route du versant d'Arvieux.

Le seuil de longueur des liaisons remplace pour l'instant le relief : avec P5, une liaison devra être coupée à son point haut quand elle franchit un col.

Puis (`census`) :

- **France seulement** : commune du point représentatif (point du réseau le plus proche du barycentre) par geo.api.gouv.fr (`communes?lat=&lon=`) ; sans commune française, le domaine est écarté. La commune nomme les domaines sans relation site.
- **OpenSkiMap** (`ski_areas.csv`, `runs.csv`, ODbL, au plus un téléchargement par jour) : recouvrement way par way (ou relation par relation) de nos domaines avec les leurs.
- **SkiRadar** (geo-feed, attribution « skiradar.fr ») : chaque station avec un bloc nordique est rattachée au domaine le plus proche à 3 km au plus.
- **Nordic France** (widget, aucune licence) : seulement avec `CensusOptions::personal_sources` (désactivé par défaut, la source n'autorise pas la réutilisation) ; rapprochement par le nom (nom du domaine, noms OpenSkiMap couvrant au moins la moitié du domaine, commune, puis noms des pistes) dans les départements de l'association, un à un d'abord.

Sorties, WGS84 :

| Fichier | Contenu | Licence |
|---|---|---|
| `data/out/france/domaines/domaines.geojson` | un MultiPolygon par domaine (enveloppe concave des pistes en Lambert-93 + 150 m), propriétés documentées dans `domaines.meta.json` | ODbL ; communes Licence Ouverte |
| `domaines_points.geojson` | point représentatif | ODbL |
| `domaines_ways.json` | ways OSM de chaque domaine | ODbL |
| `rapprochements.json` | domaines OpenSkiMap partagés ou sans aucun de nos ways ; stations SkiRadar | ODbL ; SkiRadar : attribution |
| `nordic_france.json` | stations Nordic France et domaine rapproché (seulement si `personal_sources`) | usage personnel |
| `data/out/<site>/domaines/` | mêmes fichiers, domaines touchant l'emprise du site | idem |

API : `census::build(site, &layout, &downloader)` ; `census::build_france_from` puis `census::build_site_from` pour réutiliser l'extrait. Exemple : `cargo run --release -p skimap-osm --example domaines` (`SKIMAP_PERSONAL_SOURCES=1` pour Nordic France ; la variable n'est lue que par l'exemple, `common::config` ne la connaît pas encore).

Résultats au 13/11/2026 :

| | Ce crate | Rapport 07 |
|---|---|---|
| Domaines en France | 219 (211 d'au moins 1 km nordique, 179 d'au moins 5 km), 4 415 km nordiques | OpenSkiMap : 241 (179 « operating » d'au moins 5 km, 4 939 km) |
| Noms | 95 relation site, 54 OpenSkiMap, 70 commune | 180 nommés, 61 sans nom |
| OpenSkiMap → nos domaines | 220/241 (91,3 %) ont au moins la moitié de leurs pistes dans un seul de nos domaines ; 17 n'ont aucune piste OSM (fiches Skimap.org seules) ; 44 sont partagés entre plusieurs de nos domaines (dont « Queyras - Arvieux ») | — |
| SkiRadar | 135/171 stations nordiques à moins de 3 km d'un domaine (78,9 %) ; les autres sont des stations alpines | 167/171 à moins de 10 km |
| Nordic France (usage personnel) | 90/111 (81,1 %) ; non retrouvées : Monts Jura, Plateau du Russey, Val de Vennes, Val de Morteau, Casterino, Valgaudemar, Agy, Haut Giffre… | 99/111 vers OpenSkiMap (rapprochement approché) ; mêmes absents |
| Cervières | domaine « Cervières » à part : 34,7 km nordiques + 3,2 km raquettes (Le Laus et Les Fonts réunis, liaison < 6 km) | OpenSkiMap le fond dans « Queyras - Arvieux » (nos 37,9 km sont tous dans ce domaine OpenSkiMap) |
| Arvieux | « Queyras - Arvieux » : 27,3 km nordiques | — |
| Chapelle-des-Bois | relation 13681086 : 71,0 km nordiques + 8,7 km raquettes ; voisins : Le Risoux, Bellefontaine, Morbier, Foncine / Chaux-Neuve, Lac des Rouges Truites, Mouthe, Les Rousses | OpenSkiMap : 84 km, dont 71,0 km dans notre domaine et le reste chez Mouthe, Lac des Rouges Truites et Morbier ; OpenSnowMap : 70,6 km |

Sensibilité (amas avec du nordique dans la zone France, dont ≥ 5 km ; Cervières fusionné avec Arvieux ?) :

| Paramètres | Amas | Cervières + Arvieux |
|---|---|---|
| défaut (150 m, cœur 5 km, liaison 6 km) | 688 / 510 | non |
| écart 50 m | 755 / 538 | non |
| écart 500 m (valeur OpenSkiMap) | 647 / 501 | non |
| liaison 4 km | 717 / 536 | non |
| liaison 8 km | 682 / 504 | non |
| liaison 15 km | 667 / 489 | **oui** |
| cœur 2 km / 10 km | 692 / 510 ; 693 / 514 | non |

Temps : lecture de l'extrait 3 à 5 s ; recensement 2,7 s une fois le cache rempli (regroupement 0,9 s). Le premier passage a pris 280 s, presque tout en ≈ 690 appels à geo.api.gouv.fr (≈ 2,5 par seconde observés), gardés 30 jours en cache.

Limites : la fusion à 6 km et le partage au plus proche sont des règles de réseau, pas de terrain ; le point représentatif décide du pays (un domaine franco-suisse peut tomber d'un côté) ; le rapprochement des noms Nordic France reste à valider à la main (quelques rapprochements à 0,5 sont douteux, par exemple « Saint-Pierre-en-Grandvaux » vers Saint-Laurent-en-Grandvaux).
