[← skimap-avalanche](../README.md)

# Exposition aux avalanches (P16, P17, P18, module `exposure`)

`skimap_avalanche::exposure::build(site, &layout, &downloader)` écrit `data/out/<site>/avalanche-exposure/`. Il lit les couches `terrain` (MNT 5 m) et `pistes`, et télécharge par le téléchargeur commun le MNH LiDAR HD (hauteur de canopée) et, dans les Hautes-Alpes, la couche CLPA de la DDT 05.

| Fichier | Contenu |
|---|---|
| `exposure_5m.tif` | **couche utilisateur**, GeoTIFF UInt8 Deflate, Lambert-93, même grille que `terrain/dem_5m.tif` : 1 = « terrain peu exposé (modèle) », 2 = « exposé », 3 = « très exposé », 255 = pas de donnée. Les emprises CLPA sont au moins « exposé ». |
| `exposure_model_5m.tif` | diagnostic : mêmes codes, modèle seul avant fusion CLPA (écrit si la fusion change des cellules) |
| `ates_5m.tif` | diagnostic, à ne pas afficher : classes 0 à 4 du classifieur dev8 d'AutoATES v3.0 porté (255 = pas de donnée) |
| `canopy_5m.tif` | couvert forestier, %, Float32 |
| `segments.geojson` | pistes et itinéraires coupés en tronçons de classe constante ; propriétés `osm_way_id`, `run`, `name`, `relations`, `network`, `exposure` (1 à 3 ou null), `label`, `length_m`, `way_length_m`, `way_max_exposure` ; classe échantillonnée tous les mètres, mais géométrie réduite aux sommets de la piste OSM plus un sommet interpolé à chaque changement de classe (Cervières 6 043 sommets au lieu de 71 444, 288 ko au lieu de 1,8 Mo ; Chapelle-des-Bois 14 093 au lieu de 318 967, 838 ko au lieu de 7,8 Mo ; longueurs par tronçon inchangées) |
| `summary.json` | parts des classes, kilomètres de pistes par classe, comparaison au rapport 07, validation CLPA (modèle seul et après fusion), temps |
| `avalanche-exposure.meta.json` | sources, licences, paramètres, avertissements |

**Jamais « sûr » ni « sans danger »**. Avertissements écrits dans la couche (`build::DISCLAIMERS`) : calcul automatique non vérifié sur le terrain, indépendant de la neige du jour, BERA à consulter ; « peu exposé » = aucune avalanche modélisée, même rare, à 10 m près, ni garantie ni absence de danger ; pièges de terrain, corniches, coupes forestières récentes non modélisés ; la méthode reprend un produit que ses auteurs disent « research/tutorial, not an operational ATES map ».

**Routeur hors piste** (`core::offpiste`, `OverlayKind::AvalancheExposure`) : `exposure::read_exposure_grid(&layout, site)` rend la couche en `Grid<u8>` Lambert-93 alignée sur la grille 5 m du terrain ; la table `factor` de l'`Overlay` s'indexe directement par les codes 1, 2, 3 (et 255 : au choix de l'appelant).

## Méthode

Portage Rust d'AutoATES v3.0 (instantané ISSW 2026, MIT, commit `5558c16`) et de com4FlowPy (AvaFrame, commit `d0451ce`, moteur Numba), paramètres ISSW par défaut, sans rien recaler :

1. **Forêt** (`forest`) : MNH LiDAR HD à 5 m par le WMS-R (mêmes dalles que le MNT) ; cellule « arbre » si hauteur ≥ 3 m ; couvert = part des cellules « arbre » sur 3 × 3 cellules. Choix à nous, non calibrés.
2. **Zones de départ** (`pra`) : pente de Horn exactement comme GDAL, abri au vent (médiane des angles vers les cellules à 60 m, indice de concavité), forêt ; fonctions de Cauchy et ET flou ordonné ; scénarios fréquent (seuil 0,25) et rare (0,05) ; tamis de 1000 m² avec l'algorithme de `gdal_sieve` (voisin le plus grand, en chaîne).
3. **Écoulement** (`flowpy`) : Flow-Py (D'Amboise et al. 2022), routage par cellule de départ avec persistance et exposant 8, arrêt sur la ligne d'énergie d'angle α = 30° (fréquent) ou 18° (rare), z_delta ≤ 270 m, frottement ajouté (5° à 52°) et détraînement en forêt. Choisi plutôt qu'un cône d'angle alpha seul : le cône du rapport 07 est un majorant qui ignore le terrain intermédiaire (100 % des pistes de Cervières atteignables à 18°) ; Flow-Py est le modèle d'écoulement d'AutoATES v2 et v3, ce qui rend la comparaison cellule par cellule possible. Parallèle (rayon, un chemin par cellule de départ, maxima par opérations atomiques exactes).
4. **Classes ATES 0–4** (`ates`) : classifieur dev8 (votes, planchers, score de classe 0, unité minimale de cartographie).
5. **Trois classes** : très exposé = ATES 3–4 ; peu exposé = ATES 0–1 à plus de 10 m de toute cellule atteinte par l'écoulement rare et de toute zone de départ rare (avant tamis) ; exposé = le reste. La classe 0 de la référence peut tomber dans une zone d'écoulement (rapport 07) : la règle l'interdit.
6. **Fusion CLPA** (Hautes-Alpes) : toute cellule dans une emprise CLPA passe au moins en « exposé ». La validation P18 mesure le modèle seul, avant cette fusion.

## Oracle (P16) : l'implémentation de référence

L'oracle tourne hors du dépôt, dans un environnement conda-forge (micromamba, Python 3.11, GDAL 3.12.3, numba 0.68.1, scipy 1.17.1) : AutoATES v3.0 `5558c16469173cd14725576c1a5a60b7a8e14628`, AvaFrame `d0451ce8019bf1db6b7b55a9099314a482405d8e` (extensions Cython compilées sur place), atelier ISSW `fd5067532ee80b7a0f136e87ea26934a0ecfab47`. Contrôle de l'environnement : le pipeline de l'atelier sur Connaught Creek redonne les rasters de référence publiés (pente, abri au vent, zones de départ, les trois couches d'écoulement à l'identique ; classes ATES finales à 99,84 %, AvaFrame plus récent que l'épinglage de l'atelier).

```sh
cargo run --release -p skimap-avalanche --example exposure_inputs -- cervieres     # entrées 5 m et 10 m
AUTOATES_ROOT=… AVAFRAME_ROOT=… python -I crates/avalanche/oracle/autoates_oracle.py \
    data/out/cervieres/avalanche-exposure/inputs/dem_5m.tif …/canopy_5m.tif <run_dir> --procs 10
cargo run --release -p skimap-avalanche --example oracle_compare -- cervieres 5 <run_dir> [--isolate-flowpy]
SKIMAP_AUTOATES_WORKSHOP_RUN=…/outputs/connaught cargo test --release -p skimap-avalanche --test oracle_connaught -- --ignored
```

`autoates_oracle.py` ne change aucun paramètre du modèle ; seulement l'exécution : une tuile (sinon 2 × 2 tuiles de 15 km qui se recouvrent de 12 km), `cpuCount` et `maxChunks` égaux au nombre de processus (par défaut 500 morceaux renvoyant chacun 12 rasters pleins, 275 Go à 5 m), pas de découpe par AOI, pas de rendu couleur.
