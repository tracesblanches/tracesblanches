# 07 — Recensement des domaines nordiques et zones à faible exposition aux avalanches

État au 14/11/2026. Exploration seulement : rien n'a été construit. Les fichiers téléchargés sont dans `research/raw/census_ava/`. Les scripts d'analyse sont dans `research/raw/scripts/census_ava/`.

**En bref**
- **France : aucune source n'est complète.** La plus large pour les tracés est **OpenSkiMap / OpenSkiData** : 241 domaines avec du nordique (ODbL, mise à jour quotidienne). Mais sa délimitation est fausse à Cervières, que le regroupement par proximité fusionne avec Arvieux.
- **La plus juste pour les noms et l'état est Nordic France** : 111 stations lisibles par le widget, sur 5 associations, alors que Nordic France annonce « plus de 160 sites adhérents ». Il n'y a ni coordonnées ni licence.
- **Il faut donc croiser deux sources** : la géométrie vient d'OSM, avec notre propre regroupement ; l'identité et l'état viennent de Nordic France et de l'ENJ, avec une table de correspondance tenue à la main.
- **Classe 0 de l'ATES v2 (« terrain non avalancheux »)** : elle est **optionnelle**, parce qu'elle exige une évaluation « with little to no uncertainty ». AutoATES v2.0 refusait de la cartographier automatiquement à cause de la responsabilité. La v3.0 (snapshot ISSW 2026, MIT) la calcule, mais se présente comme « research/tutorial product, not an operational ATES map ».
- **Données françaises suffisantes.** Le MNT et le MNH (hauteur de canopée) LiDAR HD sont en Licence Ouverte. Le MNT a été téléchargé sans clé à 5 m sur les deux sites (143 dalles). Pour le MNH, je n'ai vu que les tuiles d'ombrage.
- **Vérité terrain partielle.** La CLPA est restrictive chez INRAE, mais la DDT 05 la republie en Licence Ouverte : 171 emprises CLPA à Cervières. **5,8 km sur 34,5 km du réseau nordique cartographié de Cervières traversent une emprise d'avalanche connue.**
- **Le Jura n'a ni CLPA ni BERA.**

---

## Partie A — Recensement des domaines de ski de fond

### A1. France : nombre de domaines par source

| Source | Nombre | Champs utiles | Licence / accès | Cervières / Chapelle-des-Bois | Preuve |
|---|---|---|---|---|---|
| **Nordic France, widget par association** `/widget/marqueblanche/association/{slug}` | **111 stations** : ENJ 32, Nordic Alpes du Sud 32, Haute-Savoie 21, Isère 14, Savoie 12 | id, nom, km (total pistes + raquettes), pistes ouvertes/total, date du dernier damage, météo. **Pas de coordonnées.** | Aucune licence, HTML seulement | Oui : 3167 « Cervières - Izoard » (98 km), 4774 « Chapelle des Bois » (238 km) | `nf/assoc_*.html`, `nf/nf_stations.json` |
| Nordic France, déclaratif | « plus de 160 sites adhérents », « 12 associations départementales ou de massif » | — | — | — | [nordicfrance.fr](https://www.nordicfrance.fr/), [associations-adherentes](https://www.nordicfrance.fr/associations-adherentes) |
| Nordic France, sitemap des fiches station | 156 URL, dont `test` et des doublons | Les fiches renvoient un délai dépassé (90 s, 2 essais) | — | `cervieres-izoard`, `chapelle-des-bois` présentes | [sitemap](https://www.nordicfrance.fr/nordicfrance_station-sitemap.xml) |
| **OpenSkiMap / OpenSkiData** (`ski_areas.csv`) | **241 domaines FR avec du nordique** : 180 nommés, 61 générés sans nom ; 179 « operating » avec au moins 5 km ; 4 939 km cumulés | nom, statut, km nordique, point ou polygone, sites web, Wikidata, sources | ODbL, quotidien. Téléchargement automatisé autorisé « at most once per day ». Les tuiles ne peuvent pas être réutilisées. | Chapelle-des-Bois : oui (relation 13681086, 84 km). **Cervières : absent comme domaine, fusionné dans « Queyras - Arvieux » (62 km)** | [openskidata.org](https://openskidata.org), [metadata.json](https://tiles.openskimap.org/metadata.json) |
| OSM brut (Overpass, 14/11/2026 20:42Z) | 178 relations `site=piste` (tous types), 259 `landuse=winter_sports`, 8 505 ways `piste:type=nordic`, 931 relations route nordiques | — | ODbL | voir rapport 02 | `overpass_fr_counts.json` |
| bergfex, liste France | **161** | nom, km classique et skating | Propriétaire | Oui pour les deux (« Cervières », « Val de Mouthe ») | [bergfex FR](https://www.bergfex.com/frankreich/langlaufen/) |
| SkiRadar | 171 stations ayant un bloc `nordique`, sur 315 | km, itinéraires nommés (mesurés sur OSM) | Attribution « skiradar.fr » | Aucune des deux (centré sur les stations alpines) | `raw/feed.json` (rapport 03) |
| DATAtourisme (`datatourisme-place.csv`, 273 Mo) | **39** `CrossCountrySkiResort` (seulement en Bourgogne-Franche-Comté et Grand Est) + 195 points `CrossCountrySkiTrail` (surtout Apidae : Ain, Savoie, Haute-Savoie) | point, nom, commune, créateur | Licence Ouverte 2.0 | Chapelle-des-Bois : oui (« Site Nordique de Chapelle des Bois », créé par l'ENJ). Cervières : seulement « Espace ludique & Ski Hockey » | [data.gouv](https://www.data.gouv.fr/datasets/datatourisme-la-plateforme-nationale-des-donnees-touristiques), `dt_xc.json` |
| Apidae (`DOMAINE_SKIABLE`) | Non mesuré : accès Connect payant (voir rapport 03) | — | — | — | — |
| OpenSnowMap | **Pas de liste téléchargeable.** Une API non documentée `/request?bbox=` renvoie les sites (relations `site` et `landuse`). `/request?siteStats={id}` renvoie la longueur nordique en mètres. | — | ODbL | Chapelle-des-Bois : `"nordic": 70574` m. Dans la bbox de Cervières, seulement Montgenèvre et Queyras-Arvieux. | [data.html](https://www.opensnowmap.org/iframes/data.html), `osnm/req_*.json` |
| Skiinfo | Pas de liste nordique (`/france/ski-de-fond` renvoie 404). La page « stations de ski » annonce « 230 stations de ski » alpines. | — | — | Aucune des deux (rapport 02) | `skiinfo/` |
| Nordic Massif Central | Site propre : 19 fiches `station.php?id=`, n'utilise pas le widget Nordic France | — | — | — | [nordic-massif-central.com](https://www.nordic-massif-central.com/) |

**Recouvrements** (correspondance approximative par mots du nom et de la commune, à vérifier à la main) :
- **Nordic France → OpenSkiMap : 99 stations sur 111 retrouvées.** Il manque par exemple Monts Jura, Plateau du Russey, Val de Vennes, Casterino, La Draye, Valgaudemar, Agy et Haut Giffre.
- **Nordic France → bergfex : 88 sur 111.**
- **SkiRadar** : 167 de ses 171 stations nordiques sont à moins de 10 km d'un domaine nordique d'OpenSkiMap.

**Limites d'OpenSkiMap en France :**
- 61 domaines n'ont pas de nom : ils sont générés à partir de pistes isolées et totalisent 824 km.
- Il y a des doublons : Les Rousses ×2, Bellefontaine ×2, Col de Porte ×2, Les Entremonts ×2.
- 191 domaines sur 241 n'ont qu'un **point**. Il n'y a un polygone que si OSM contient un `landuse=winter_sports`.

**Quelle source est la plus complète ?**
- **En nombre et en géométrie : OpenSkiMap** (241), puis bergfex (161), puis Nordic France (111 lisibles, « plus de 160 » annoncés).
- **En autorité sur le nom, le périmètre commercial et l'état du jour : Nordic France**, seule à donner un statut pisteur par piste. Mais elle n'a ni coordonnées ni licence, et il manque Vosges, Pyrénées, Drôme et Massif central dans les slugs que j'ai trouvés.
- **Estimation du total France : 150 à 200 domaines nordiques gérés.** C'est une fourchette de mon cru, entre les 160 adhérents annoncés et les 179 domaines OSM « operating » d'au moins 5 km, et elle reste **non vérifiée**.

### A2. Europe et monde

| Source | Couverture | Nombre | Licence | Preuve |
|---|---|---|---|---|
| **OpenSkiData** (monde) | 83 pays | 12 435 domaines, dont **5 817 avec du nordique** ; 100 256 km nordiques cumulés ; 3 573 domaines d'au moins 5 km. Par pays : US 994, NO 948, RU 632, FI 605, DE 478, CA 447, SE 409, AT 258, **FR 241**, CH 181, IT 132, CZ 127 | ODbL + Skimap.org + Who's On First + Mapterhorn (attribution exigée) | [metadata.json](https://tiles.openskimap.org/metadata.json) (`"nordic": 5817`), `osm_ski_areas.csv` |
| Norvège — **Sporet** (Skiforeningen) | National | Couche ArcGIS `Destinasjoner_singel` : **860** destinations ; `Loypetype` : 36 130 objets | Pas de licence ouverte affichée (« Skiforeningen ») ; couches `Sporet_paid` séparées | `https://ags.sporet.no/arcgis/rest/services/Sporet_simple/MapServer` (`returnCountOnly`) |
| Suède — **Skidspår.se** | National, participatif | « över 1 600 anläggningar och närmare 5 000 spår » | Non indiquée | [om-skidspar](https://www.skidspar.se/om-skidspar/om-oss) |
| Suisse — **Langlauf.ch** (Loipen Schweiz) | National | **198 domaines** (10 pages, 20 par page sauf la dernière à 18). bergfex CH : 170. Aucune couche des pistes de fond sur geo.admin (seulement les itinéraires raquettes et randonnée hivernale). | Propriétaire | [langlaufgebiete](https://www.langlauf.ch/de/langlaufgebiete), `national/geoadmin_layers.json` |
| Allemagne | Pas de portail national trouvé | bergfex DE : 319 | — | [bergfex DE](https://www.bergfex.com/deutschland/langlaufen/) |

**Cervières et Chapelle-des-Bois dans OpenSkiMap :**
- **Chapelle-des-Bois : présent**, polygone dérivé de la relation OSM `site=piste` 13681086 (16 relations de piste), 84 km.
- **Cervières : absent comme domaine.** Les 33 runs nordiques au nord de 44,82° N (34,2 km) sont rattachés à « Queyras - Arvieux » (relation 6346133), alors qu'**aucun n'est membre de cette relation** (vérifié). Ils ont été absorbés par proximité de proche en proche (0,5 km) à travers le col d'Izoard.

### A3. Méthode pour délimiter et afficher les domaines

**Comment OpenSkiMap regroupe les pistes** (fichier `src/clustering/SkiAreaClusteringService.ts`, lignes 68-69 et 364-421, [dépôt](https://github.com/russellporter/openskidata-processor)) :
1. Les objets situés **dans** un polygone `landuse=winter_sports` lui sont rattachés. Le domaine est supprimé si plus de 50 % de sa longueur appartient à une seule relation `site=piste` (lignes 709-744).
2. Les **objets proches** sont ensuite rattachés de proche en proche dans un tampon : `maxDistanceInKilometers = 0.5` (ligne 68, puis `ST_Buffer`).
3. Les domaines Skimap.org situés à moins de 0,25 km sont fusionnés (`maxSkiAreaMergeDistanceInKilometers = 0.25`).
4. Les pistes restantes sont rattachées aux domaines Skimap.org.
5. Pour les pistes qui restent seules, un **domaine sans nom** est généré (`name: null`, ligne 1080).

**Géométrie produite :** un **point** placé près du barycentre des pistes (`skiAreaGeometry`, ligne 1371). Il n'y a un polygone que si la source est un `landuse`. Licence du code : Apache-2.0 pour openskimap.org. L'API GitHub ne détecte pas de licence pour openskidata-processor : **non vérifié**.

**Recommandation pour l'app :**
- **Géométrie** : à partir des pistes OSM (`planet_pistes.osm.gz` d'OpenSnowMap, ou `runs.geojson` d'OpenSkiData, une fois par jour au plus). Regrouper soi-même :
  1. d'abord par les relations `site=piste`, sinon par `landuse` ;
  2. **ensuite seulement** par proximité, avec un seuil plus court (par exemple 150 m) et **sans franchir** une piste `hike`/route ou un col. Cette règle évite la fusion Cervières–Arvieux ; c'est ma proposition, à tester.
  3. Afficher l'enveloppe concave des pistes, avec un tampon de 100 à 200 m.
- **Noms, kilométrage officiel et état** : table de correspondance tenue à la main entre domaine OSM et id Nordic France (environ 150 lignes), plus l'API Geotrek de l'ENJ pour le Jura. Il faut un accord, puisqu'aucune licence n'est donnée (rapport 03).
- **Cadence** :
  - géométrie : rebuild hebdomadaire ou mensuel (OSM bouge peu ; OpenSkiData est reconstruit chaque jour) ;
  - table d'identité : une fois par saison ;
  - état des pistes : quotidien en saison. Le widget porte une seule date de damage par secteur, et la fréquence de mise à jour en saison reste **non vérifiée**.

---

## Partie B — Zones de faible exposition aux avalanches définies par algorithme

### B1. Définitions en usage

| Système | Classe « sans avalanche » | Formulation exacte | Source |
|---|---|---|---|
| **ATES v2** (Statham & Campbell, NHESS 25, 1113–1137, 2025) | Classe 0 « Non-avalanche terrain » | « Determining a Class-0 (Non-avalanche terrain) rating requires high confidence in the assessment and can have little to no uncertainty. For this reason, the use of ATES Class 0 is optional, and Class 1 can include Class-0 terrain. » | [PDF p. 1115](https://nhess.copernicus.org/articles/25/1113/2025/nhess-25-1113-2025.pdf) |
| ATES v2, seuils SIG (Tableau 6, d'après Campbell & Gould 2013) | Classe 0 | Forêt ouverte : « 99 % ≤ 20° » ; clairsemée (« gladed ») : « 99 % ≤ 25° » ; dense : « 99 % ≤ 30° ». Pentes moyennées sur 20 à 30 m dans la ligne de pente. Densités (Tableau 5) : ouverte < 100 tiges/ha, clairsemée 100 à 1000, dense > 1000. | idem p. 1121 |
| ATES v2, modèle de communication (cité par Alberta Parks) | Classe 0 | « No known exposure to avalanches. Very low-angle or densely forested slopes located well away from avalanche paths, or designated trails/routes with no exposure to avalanches. » | [Alberta Parks](https://albertaparks.ca/albertaparksca/advisories-public-safety/outdoor-safety/winter-safety/ates-disclaimer/kananaskis-country-avalanche-terrain-ratings-scale-ates/?park=kc) |
| avalanche.org (États-Unis) | Classe 0 | Même texte. La légende d'une image dit « safe from avalanche terrain regardless of conditions » : **contre-exemple à ne pas suivre** | [avalanche.org](https://avalanche.org/avalanche-encyclopedia/terrain/avalanche-terrain-exposure/avalanche-terrain-exposure-scale-ates/) |
| **France — EETA** (ANENA + INRAE, PNR du Queyras, 4 vallons de l'est du Queyras, cartographie manuelle 2024) | Classe 0 « pas d'avalanche » | « Pas de danger d'avalanche, même occasionnel. Toutes options possibles. Lits de ruisseaux, murets, talus ». Avertissement : « La carte ne remplace pas une analyse personnelle… La consultation et la compréhension du Bulletin du Risque d'Avalanche (BRA) sont indispensables » | [pnr-queyras.fr/application-eeta](https://www.pnr-queyras.fr/application-eeta/), [FFRandonnée, 14/12/2024](https://www.ffrandonnee.fr/s-informer/actualites/securisation-du-ski-de-randonnee-dans-le-queyras-avec-la-methode-eeta) |
| **Suisse — CAT / ATH** (SLF, White Risk) | Pas de classe 0 : zones de départ en rouge (30 à 50°), écoulement en jaune et bleu pour une avalanche de taille 3 | La page ne dit pas qu'une zone non colorée est sûre (aucun texte d'avertissement sur la page) | [White Risk CAT](https://content.whiterisk.ch/en/help/maps/classified-avalanche-terrain-cat), [ISSW 2024 P12.1](https://arc.lib.montana.edu/snow-science/objects/ISSW2024_P12.1.pdf) |
| Autriche | AutoATES Austria (Huber et al., ISSW 2023, p. 929-936), cité dans ISSW 2024 P12.1 | Texte non lu : **non vérifié** | — |
| **Skitourenguru** (FR) | Pas de « sûr ». Vert = « risque modéré » | « Notez que les résultats sont par principe incertains ! … Skitourenguru GmbH ne donne aucune garantie et par conséquent décline toute responsabilité… ne peut être utilisé que comme source d'informations complémentaire » | bundle `js_locales` de skitourenguru.com (`raw/stg/`) |
| CAIC (Colorado) | Pas de carte ATES CAIC trouvée. onX a publié des zones ATES avec « 0-Non Avalanche » (selon un résultat de recherche) | **non vérifié** | — |

### B2. Algorithmes

| Brique | Principe | Entrées | Précision publiée | Licence, langage |
|---|---|---|---|---|
| **AutoATES v1.0** (Larsen et al., Nat. Hazards 103, 2829–2847, 2020) | Seuils ATES + zones de départ (PRA) + longueur d'écoulement TauDEM ; « 10-m resolution » pour toute la Norvège | MNT 10 m, forêt | Qualitative : pour le terrain ouvert, « vast majority » des couloirs dans la même classe ; résultats mitigés en forêt (données de végétation insuffisantes) | Logiciel propriétaire (selon Toft et al. 2024) — [résumé](https://ideas.repec.org/a/spr/nathaz/v103y2020i3d10.1007_s11069-020-04104-7.html) |
| **AutoATES v2.0** (Toft et al., NHESS 24, 1779–1793, 2024) | PRA flou (pente, abri au vent, forêt ; fonctions de Cauchy ; seuil 0,15), puis **Flow-Py** avec arrêt à un angle alpha, puis reclassement en forêt | MNT et forêt entre 5 et 30 m ; forêt en tiges/ha, couvert % ou surface terrière | **F1 : Bow Summit 64 → 77 %, Connaught Creek 40 → 71 %** | **GPL-3.0**, Python ([AutoATES-v2.0](https://github.com/AutoATES/AutoATES-v2.0)) |
| Validation (Sykes et al., NHESS 24, 947–971, 2024) | Comparaison à 2 cartes de référence issues de 3 experts | MNT lidar, photogrammétrie, CDEM 5 à 26 m | Accord **74,5 %** (Connaught) et **84,4 %** (Bow) ; cartes manuelles **76,1 %** et **84,8 %** ; « 74.5 % ± 1 % for all DEMs » | Article CC BY 4.0 — [PDF](https://nhess.copernicus.org/articles/24/947/2024/nhess-24-947-2024.pdf) |
| **AutoATES v3.0** (snapshot ISSW 2026) | MNT + couvert forestier → PRA typique et rare → **com4FlowPy** (α 30° et 18°) → **classes 0 à 4** | ALOS 30 m + couvert Sentinel-2 0-100 % ; « Ruggedness is off on ~30 m surfaces » | Non publié ici ; « research/tutorial product, not an operational ATES map » | **MIT**, Python ; dépôt de développement privé — [autoATES-v3.0-issw](https://github.com/AutoATES/autoATES-v3.0-issw) |
| **Classe 0 dans la v3** (`dev8.py`, lignes 46-47, 95-108, 197-200) | Score = moyenne de (14° − pente moyenne sur 300 m)/14, (20° − pente)/20, couvert/60 %, distance à l'écoulement/200 m, distance à une PRA/500 m. **Classe 0 si score ≥ 0,60, appliquée après les planchers.** | — | — | Ma lecture du code : une cellule plate et boisée **dans** une zone d'écoulement peut tomber en classe 0 (5 facteurs : 1, 1, 1, 0, 1 → 0,8). **À vérifier avec les auteurs** avant toute réutilisation. |
| **Flow-Py v1.0** (D'Amboise et al., GMD 15, 2423–2439, 2022) | Routage sur le MNT, arrêt selon un concept d'angle d'écoulement | MNT + zones de départ + au moins 4 paramètres | Tests sur pentes génériques : RMSE 4×10⁻⁵ face à la solution géométrique | GPL-3.0 ([avaframe/FlowPy](https://github.com/avaframe/FlowPy)) ; com4FlowPy est intégré à **AvaFrame, EUPL-1.2** ([OpenNHM/AvaFrame](https://github.com/OpenNHM/AvaFrame)) |
| **PRA** (Bühler et al., NHESS 18, 3235–3251, 2018) | Comparaison d'algorithmes de zones de départ | swissALTI3D ramené à 5 m (« sufficient ») ; forêt binaire | POD et POFD : pente seule 98,69 / 28,89 ; Veitinger 2016 96,74 / 20,50 ; OBIA « frequent » 95,39 / 15,88 | [article](https://nhess.copernicus.org/articles/18/3235/2018/) |
| **Méthode alpha-bêta** (Lied & Bakkehøi 1980) | Portée statistique | — | Citée par CATv2 et AutoATES | — |
| **Forêt** | « one of the most important parameters for ATES classification » (Toft 2024). AutoATES v2 l'utilise dans la PRA puis dans un reclassement final. CAT suisse : forêt dense ignorée, forêt ouverte traitée comme terrain libre. | — | — | — |
| **Pièges de terrain** | Paramètre manuel de l'ATES v2 (Tableau 3). **Non modélisés** par AutoATES v2 ou v3 d'après les documents lus : **non vérifié** dans le code complet | — | — | — |

**Coût de calcul de référence** (Toft et al. 2024, p. 1790-1791) : « We executed the Flow-Py algorithm across all of Norway on an Amazon Web Services Elastic Cloud Compute Instance (AWS EC2 c6g.metal), which took 30 d to complete at a cost of USD 1600 ». La Norvège fait 385 207 km². La résolution de ce calcul n'est pas dite dans le passage ; le modèle utilise « a 10 m DEM as the default » : **non vérifié** pour ce calcul.

### B3. Données pour la France, et estimation du calcul

| Donnée | Constat vérifié | Licence |
|---|---|---|
| **MNT LiDAR HD** (dalles de 1 km, 50 cm ; version 5 m annoncée) | WFS `IGNF_LIDAR-HD_METADONNEE:metadata` : **Cervières, 78 dalles** (vol d'août 2021, édition du 15/03/2025) ; **Chapelle-des-Bois, 65 dalles** (vol oct.-nov. 2022, édition du 21/07/2025). Les 143 dalles ont été téléchargées **sans clé**, rééchantillonnées à 5 m par WMS (`WIDTH=200`). 12 requêtes ont d'abord échoué (« LayerNotDefined ») puis ont réussi après de nouveaux essais. | Licence Ouverte (`lov2`), avec des zones ZICAD masquées — [data.gouv MNT](https://www.data.gouv.fr/datasets/mnt-lidar-hd) |
| **MNH LiDAR HD** (hauteur de canopée) | Tuiles d'ombrage présentes au Risoux (forêt visible) et à Cervières | `lov2` — [data.gouv MNH](https://www.data.gouv.fr/datasets/mnh-lidar-hd) |
| RGE ALTI 1 m / 5 m | « Modèle Numérique de Terrain (MNT) aux pas de 1 m et 5m » | `lov2`, mis à jour le 01/10/2026 — [data.gouv](https://www.data.gouv.fr/datasets/rge-alti-r) |
| BD Forêt V2 / masque forêt 2021-2023 | Formations et essences (32 postes) ; couches WMTS présentes (rapport 04) | `lov2` — [data.gouv](https://www.data.gouv.fr/datasets/bd-foret-r) |
| swissALTI3D | 0,5 m ou 2 m, mis à jour tous les 6 ans ([swisstopo](https://www.swisstopo.admin.ch/en/height-model-swissalti3d)). Les conditions des géodonnées libres permettent l'usage commercial avec citation de la source ([conditions](https://www.swisstopo.admin.ch/en/terms-of-use-free-geodata-and-geoservices)). Le champ STAC `license` vaut « proprietary » : le rattachement explicite de swissALTI3D aux géodonnées libres est **non vérifié**. | — |

**Pré-filtre géométrique que j'ai calculé** (MNT LiDAR HD à 5 m, sans forêt, sans routage). Les PRA sont approchées par les pentes de 30 à 60° à 10 m ; un point de piste est dit « atteignable » si une PRA située à moins de 3 km le domine sous un angle ≥ α. C'est un majorant ; **ce n'est pas un auto-ATES**.

| | Cervières (bbox 44,82–44,88 N / 6,70–6,82 E) | Chapelle-des-Bois (46,57–46,64 N / 6,06–6,16 E) |
|---|---|---|
| Surface ≥ 30° | 38,1 % | 2,3 % |
| Points de piste nordique (pas de 25 m) | 1 465 (~36,6 km) | 5 339 (~133,5 km) |
| … atteignables avec α = 18° (scénario rare) | **100 %** | **8,7 %** |
| … avec α = 24° | 95,1 % | 3,9 % |
| … avec α = 30° (scénario typique) | 69,0 % | 1,0 % |

Conséquence : dans le Jura, plus de 90 % du réseau de fond est candidat à la classe 0, et le reste se localise précisément. À Cervières, aucun tronçon n'est classable 0 sans le routage et la forêt.

**Estimation du coût**, en partant du chiffre norvégien : 1 600 USD / 385 207 km² ≈ **4,2 USD pour 1 000 km²**, soit 0,12 vCPU·h/km², à la résolution du calcul norvégien (supposée 10 m). Passer à 5 m multiplie le nombre de cellules par 4 et la longueur des trajets par environ 2 : facteur ≈ 8. Ce facteur est **mon hypothèse, non mesurée**.

Surfaces de calcul. Le réseau nordique OSM de France (11 300 ways, 4 876 km, Overpass du 14/11/2026 20:55Z) a été tamponné par transformée de distance sur une grille de 100 m (`buf2.py`) :

| Emprise | km² | Si résolution 10 m (ratio Norvège) | Si 5 m (×8, hypothèse) |
|---|---|---|---|
| Pistes nordiques FR, tampon 500 m | 2 991 | ≈ 12 USD, ≈ 0,2 jour sur un c6g.metal | ≈ 99 USD, ≈ 2 j |
| tampon 1 km | 5 548 | ≈ 23 USD, 0,4 j | ≈ 184 USD, 3,5 j |
| tampon 2 km | 11 693 | ≈ 49 USD, 0,9 j | ≈ 389 USD, 7 j |
| **tampon 3 km** (emprise de calcul conseillée) | **19 024** | **≈ 79 USD, 1,5 j** | **≈ 630 USD, 12 j** |
| Alpes françaises (part FR de la Convention alpine, 21,4 % de 190 711 km² ([alpconv.org](https://alpconv.org/en/home/organisation/contracting-parties))) | ≈ 40 800 | ≈ 170 USD, 3,2 j | ≈ 1 360 USD, 25 j |
| Toutes les Alpes (190 711 km²) | 190 711 | ≈ 790 USD, 15 j | ≈ 6 340 USD, 119 j |

Ces coûts supposent que le coût du calcul est proportionnel à la surface. C'est pessimiste pour le Jura, les Vosges et le Massif central, où il y a peu de zones de départ, donc peu de trajets à simuler ; le coût réel y serait plus faible, mais **cela n'a pas été mesuré**.

Le calcul doit couvrir l'amont des pistes : tampon de calcul de 2 à 3 km, tampon d'affichage de 500 m. Les itinéraires de SRN hors OSM (GTJ, 108 topos de skirandonneenordique.com) ajouteraient quelques centaines de km² : **non chiffré**.

Mesure à faire avant de choisir l'outil : chronométrer com4FlowPy (moteur numba) sur **une** zone alpine dense comme Cervières, qui est le cas dominant, et non sur le Jura plat. Le calcul se fait hors ligne, une fois ; il n'est pas dans l'app.

### B4. Vérité terrain française

| Jeu | Couverture | Accès / licence | Constat |
|---|---|---|---|
| **CLPA** (INRAE) | « une grille régulière unique pour les Alpes et les Pyrénées » : **pas le Jura**. Le site annonce « 850 000 ha cartographiés, 25 000 emprises d'avalanches » | Site INRAE : reproduction réservée à un « usage personnel excluant tout usage à des fins publicitaires et/ou commerciales et/ou d'information » ; l'autorisation d'INRAE est nécessaire | [données publiques CLPA](https://www.avalanches.fr/donnees-publiques-clpa) |
| **CLPA vectorisée, republiée par la DDT 05** (`L_ALEA_AVALANCHE_005`) | Hautes-Alpes : 5 882 polygones (sources CLPA 1 038, PPR-CLPA 723, PPR 3 715…). **Cervières : 171 emprises CLPA + 1 PPR, 29,6 km²** (168 « niveau fort ») | **Licence Ouverte 2.0** sur data.gouv (le droit de la DDT à republier la CLPA en Licence Ouverte n'est **pas vérifié**) | [data.gouv DDT05](https://www.data.gouv.fr/datasets/ddt05-risque-naturel-alea-avalanche-dans-les-hautes-alpes-shp). **Intersection avec le réseau nordique OSM de Cervières : 5,78 km sur 34,5 km** (liaison Le Laus – Les Fonts 1,38 km, Chemin du Sioux 1,14, Le Cancel 1,03, Izoard 0,85, La Pinatelle 0,42, L'Aigalan 0,34). |
| CLPA en Auvergne-Rhône-Alpes (DREAL) | Mailles avec liens vers les fiches PDF | « License Not Specified » ; le lien de téléchargement renvoie une page HTML | [data.gouv](https://www.data.gouv.fr/datasets/carte-de-localisation-des-phenomenes-davalanche) |
| **EPA** | 3 900 sites observés dans les Alpes et les Pyrénées ; 109 107 événements | Listes en PDF par département, mêmes conditions INRAE. Le service GeoServer de la carte était « indisponible » le 14/11. | [données publiques EPA](https://www.avalanches.fr/donnees-publiques-epa) |
| Jura | Pas de CLPA, pas d'EPA, pas de BERA (rapport 04) | — | Validation possible seulement avec la pente LiDAR et des avis locaux (ENJ, pisteurs) |

INRAE précise que ces dispositifs « ne sont donc pas adaptés aux besoins des pratiquants de la montagne… lesquels doivent utiliser… notamment le BRA » ([avalanches.fr](https://www.avalanches.fr/)).

### B5. Formulation et responsabilité

- **Formules des autres acteurs :**
  - AutoATES v2 : « there is significant liability associated with deeming an area safe from avalanche hazard » (Toft 2024, p. 1791).
  - Alberta Parks : « Due to the computer modelling nature of the map, there may be errors… Detailed decisions… must still be evaluated by users in the field ».
  - EETA Queyras : « ne remplace pas une analyse personnelle » et le BRA est « indispensable ».
  - Skitourenguru : « résultats… par principe incertains », « décline toute responsabilité ».
- **Formulation proposée** : jamais « sûr » ou « sans danger ». Plutôt :
  - « Terrain peu exposé (modèle) : pas de pente raide détectée au-dessus, d'après le relief LiDAR IGN » ;
  - avec la mention « calcul automatique, non vérifié sur le terrain, indépendant de la neige du jour ; consultez le BERA ».
- **Jurisprudence française** : je n'ai trouvé **aucune décision** sur la responsabilité d'un éditeur de carte, de topo ou d'application pour une information d'avalanche (deux recherches, résultats limités aux guides et moniteurs, par exemple Cass. civ. 2e, 24/04/2003, n° 01-00.450, sur un topo-guide d'escalade). **Non vérifié.** Une consultation juridique serait nécessaire.

---

## Ce que ça permet dans l'app

1. **Carte de tous les domaines nordiques**, hors ligne. Contours calculés depuis OSM (ODbL, attribution OpenSkiData si on réutilise leurs fichiers). Fiche par domaine reliée à Nordic France par une table maison. Coût : un build quotidien, quelques Mo par massif.
2. **Calque « terrain peu exposé (modèle) »** le long des pistes et des itinéraires de SRN, en trois teintes (peu exposé / sous une pente raide / dans une emprise CLPA connue), jamais « sûr ».
   - **Le Jura est le cas favorable** : 91 % des points sont hors du cône α = 18°, calcul léger : le pré-filtre ci-dessus a tourné en quelques minutes sur un poste, pour les deux sites.
   - **Cervières demande le pipeline complet** (forêt LiDAR MNH, com4FlowPy) et une revue locale. Les 5,8 km en emprise CLPA peuvent s'afficher dès maintenant à partir du jeu DDT 05 (Licence Ouverte).
3. **Alerte de franchissement** : « Ce tronçon traverse une zone d'avalanche historique (CLPA) », couplée au niveau du BERA du massif (Queyras). Il n'y a pas de BERA dans le Jura.

## Non vérifié
- Le nombre exact de domaines Nordic France. Les slugs des associations Vosges, Pyrénées, Drôme et Ariège, et Massif central (qui a son propre site), n'ont pas été trouvés. La licence et le droit de réutiliser le widget.
- La correspondance Nordic France ↔ OSM ↔ bergfex, faite par mots du nom et donc approximative.
- La licence du code d'openskidata-processor.
- Les licences de Sporet et de Skidspår.se. Un portail allemand national.
- La résolution du calcul norvégien à 30 jours. Le facteur ×8 entre 10 m et 5 m.
- Le comportement de la classe 0 dans AutoATES v3 (lecture du code seulement). Le texte d'AutoATES Austria.
- Une carte ATES publiée par le CAIC.
- Le droit de la DDT 05 à republier la CLPA en Licence Ouverte. La CLPA des Alpes-de-Haute-Provence et des Savoies non téléchargée.
- swissALTI3D nommément en géodonnée libre.
- Toute jurisprudence française sur la responsabilité d'une app ou d'une carte d'avalanche.
- Le pré-filtre géométrique n'utilise ni la forêt, ni le routage, ni les pentes hors bbox (bord de Cervières).
