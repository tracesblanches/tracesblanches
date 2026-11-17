# skimap-osm, P3 : domaines de fond, France

[← skimap-osm](../README.md)

Regroupement maison (`domains::cluster`), lancé sur toutes les pistes de la zone France de l'extrait (≈ 33 000 ways, 0,9 s) :

1. **Réseau** : graphe des pistes (nordic, hike, snowshoe) coupé aux jonctions ; deux réseaux à moins de 150 m sont reliés (OpenSkiMap : 500 m de proche en proche).
2. **Relations `site=piste`** : chaque relation site avec du nordique est une graine ; un way sert la relation la plus précise qui le contient (« Le Risoux » dans « Les Rousses » : une relation dont les ways sont un sous-ensemble strict d'une autre, ou qui en est membre, est un sous-site). Deux relations site ne fusionnent jamais.
3. **Cœurs de réseau** : un pont (arête dont le retrait coupe le réseau) est une liaison quand chaque côté pèse au moins 5 km ; sans ces ponts, les morceaux d'au moins 5 km sont des cœurs. Un cœur sans relation site devient une graine.
4. **Partage** : Dijkstra multi-sources sur le réseau, chaque nœud va à la graine la plus proche.
5. **Fusion** : deux cellules voisines (pas deux relations site) fusionnent si la liaison qui les relie fait moins de 6 km (Le Laus – Les Fonts sur le jeu de test alpin) ; une liaison plus longue est un passage entre vallées (du jeu de test alpin à Brunissard par l'Izoard : 13,2 km).
6. **Cohérence des relations** : une relation de piste de 25 km au plus coupée entre deux domaines va entière au domaine qui en a le plus ; la coupure de l'Izoard tombe ainsi au col, entre la piste « Izoard » du jeu de test alpin et la route du versant d'Arvieux.

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

API : `census::build(site, &layout, &downloader)` ; `census::build_france_from` puis `census::build_site_from` pour réutiliser l'extrait. Exemple : `cargo run --release -p skimap-osm --example domaines` (`SKIMAP_PERSONAL_SOURCES=1` pour Nordic France, lu par `common::config::Config::personal_sources`).

Résultats au 13/11/2026 :

| | Ce crate | Rapport 07 |
|---|---|---|
| Domaines en France | 219 (211 d'au moins 1 km nordique, 179 d'au moins 5 km), 4 415 km nordiques | OpenSkiMap : 241 (179 « operating » d'au moins 5 km, 4 939 km) |
| Noms | 95 relation site, 54 OpenSkiMap, 70 commune | 180 nommés, 61 sans nom |
| OpenSkiMap → nos domaines | 220/241 (91,3 %) ont au moins la moitié de leurs pistes dans un seul de nos domaines ; 17 n'ont aucune piste OSM (fiches Skimap.org seules) ; 44 sont partagés entre plusieurs de nos domaines (dont « Queyras - Arvieux ») | — |
| SkiRadar | 135/171 stations nordiques à moins de 3 km d'un domaine (78,9 %) ; les autres sont des stations alpines | 167/171 à moins de 10 km |
| Nordic France (usage personnel) | 90/111 (81,1 %) ; non retrouvées : Monts Jura, Plateau du Russey, Val de Vennes, Val de Morteau, Casterino, Valgaudemar, Agy, Haut Giffre… | 99/111 vers OpenSkiMap (rapprochement approché) ; mêmes absents |
| Jeu de test alpin | domaine à part : 34,7 km nordiques + 3,2 km raquettes (Le Laus et Les Fonts réunis, liaison < 6 km) | OpenSkiMap le fond dans « Queyras - Arvieux » (nos 37,9 km sont tous dans ce domaine OpenSkiMap) |
| Arvieux | « Queyras - Arvieux » : 27,3 km nordiques | — |
| Jeu de test jurassien | relation 13681086 : 71,0 km nordiques + 8,7 km raquettes ; voisins : Le Risoux, Bellefontaine, Morbier, Foncine / Chaux-Neuve, Lac des Rouges Truites, Mouthe, Les Rousses | OpenSkiMap : 84 km, dont 71,0 km dans notre domaine et le reste chez Mouthe, Lac des Rouges Truites et Morbier ; OpenSnowMap : 70,6 km |

Sensibilité (amas avec du nordique dans la zone France, dont ≥ 5 km ; Jeu de test alpin fusionné avec Arvieux ?) :

| Paramètres | Amas | Jeu de test alpin + Arvieux |
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
