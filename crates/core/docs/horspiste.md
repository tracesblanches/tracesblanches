# skimap-core, C3 et C4 : routage hors piste

[← skimap-core](../README.md)

Module `offpiste` (C3, #35 ; banc d'essai Pixel 8, C4, #36). Résumé de la documentation du module et du compte rendu du ticket #35.

## Données

Le pipeline prépare une fois par site des rasters compacts sur la grille Lambert-93 de 5 m de `skimap-terrain`, dans `data/out/<site>/offpiste/` (`skimap_core::offpiste::prepare::prepare(site, &layout, &dossiers_neige)`, fonction `pipeline`, appelée par la commande `prepare` de l'exemple `offpiste_bench` ; métadonnées `offpiste.meta.json` au schéma commun `LayerMeta`) :

| Fichier | Type | Contenu |
|---|---|---|
| `elevation_5m.skr` | `u16` | altitude, décimètres (IGN69), 65535 = absente |
| `slope_5m.skr` | `u8` | pente, `floor(2 × degrés)`, 255 = absente |
| `zones_5m.skr` | `u8` | classe de zones (indice dans `zone_classes.tsv`) |
| `zone_classes.tsv` | texte | point représentatif et identifiants des zones de chaque classe |
| `pistes_5m.skr` | `u8` | 0 hors réseau, 1 itinéraire balisé non damé, 2 piste nordique damée |
| `snow_<date>.skr` | `u8` | classes de neige du jour (25 m), date lue dans `snow.meta.json` (`valid_from`) ; au plus 7 dates (`SNOW_DATES_KEPT`) : celles de l'exécution, puis les plus récentes de l'`offpiste.meta.json` précédent ; les autres fichiers `snow_<date>.skr` sont supprimés |
| `exposure_5m.skr` | `u8` | exposition aux avalanches de la couche `avalanche-exposure` (fusion CLPA) : 1 terrain peu exposé (modèle), 2 exposé, 3 très exposé, 255 absente ; écrit par `prepare_with_exposure` (`skimap offpiste-prepare` le lit avec `read_exposure_grid` de la crate avalanche) |

Format `SKR1` : en-tête de 72 octets (magic, type, taille, EPSG, valeur absente, géotransformation GDAL), puis les cellules. Les fichiers sont écrits en différences le long des lignes (bit 0x80 de l'octet de type ; premier point de chaque ligne tel quel, puis différence avec le voisin de gauche modulo 2^8 ou 2^16, octets faibles de la ligne puis octets forts) : sans perte, et le gzip du pack passe de 17,1 à 8,6 Mo pour l'altitude de Cervières (10,5 à 5,7 Mo à Chapelle-des-Bois), de 7,2 à 5,8 Mo pour la pente ; les rasters de classes ne changent pas. Le lecteur lit les deux formes ; un lecteur qui ne connaît que la forme brute refuse la nouvelle (type inconnu) au lieu de la mal lire. Contrôle : fichiers de Rust identiques octet pour octet à la transformation faite par un oracle Python sur les fichiers bruts. La lecture n'a besoin que de `std` : le téléphone n'embarque ni décodeur GeoTIFF ni pile HTTP. Les rasters gardent les entrées du coût, pas un coût figé : pente maximale et poids sont des paramètres de la requête (`CostParams`), les zones dépendent de la date (`zone_policy`), la neige change chaque jour.

## Zones à une date

Une classe de zones est un ensemble distinct de zones contenant un centre de cellule ; `zone_policy` l'évalue une fois avec `zone_eval::evaluate` à son point représentatif : accès interdit, classe bloquée ; sortie des itinéraires interdite (APPB du Risoux du 15/12 au 30/06, districts francs suisses), bloquée hors des itinéraires balisés ; « éviter » (Biodiv'sports en saison), facteur `avoid_factor` ; « interdit par endroits », signalé seulement.

## Coût et recherche

Coût d'un pas entre deux des 8 voisines, en secondes pondérées :

```text
montée   (dz ≥ 0) : (L / v_plat + dz / v_montée) × (f_a + f_b) / 2
descente (dz < 0) : L / v_plat × descente(|dz| / L) × (f_a + f_b) / 2
```

Facteur de cellule `f` : infini si la pente atteint `max_slope_deg` (30° par défaut), si le relief manque ou par la politique de zones ; dévers croissant au-delà de `steep_from_deg` ; classe de neige ; réseau de pistes (damée × 0,6, balisée) ; zones à éviter ; surcouches `Overlay`. Exposition aux avalanches par défaut (`ExposureFactors`, `load_exposure_overlay`) : « exposé » ×3, « très exposé » bloqué sauf si l'utilisateur l'autorise (facteur fini), « terrain peu exposé (modèle) » et sans donnée ×1 ; « peu exposé » est une sortie de modèle, jamais « sûr ». Exemple : `SKIMAP_OFFPISTE_EXPOSURE=off`, `SKIMAP_EXPOSURE_HIGH=<facteur>`, `SKIMAP_EXPOSURE_EXPOSED=<facteur>`. Pas de diagonale qui coupe le coin d'une cellule bloquée. Recherche A* sur une fenêtre (emprise des deux extrémités élargie de `window_margin_m`), heuristique minorante et cohérente : coût égal à Dijkstra (tests et données réelles).

## Preuves (`proofs`)

```sh
cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- prepare <site> [dossiers snow...]
cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- proofs
```

Sorties : `data/out/<site>/routes/offpiste_*.geojson` (décrits dans `routes.meta.json`). Contrôles indépendants du raster sur chaque cellule : pente du `slope_5m.tif` d'origine < 30°, `zone_eval::evaluate` au centre de chaque cellule et au milieu de chaque segment (aucun accès interdit, aucune cellule hors piste là où la sortie des itinéraires est interdite).

Avec l'exposition aux avalanches par défaut (15/11/2026) ; entre parenthèses, sans elle (mêmes itinéraires qu'avant, cellules « exposé » comptées sur la même couche) :

| Itinéraire (15/02/2026 09 h UTC sauf août) | Longueur | D+ | Coût | Cellules hors piste / damée | Cellules exposé / très exposé | Hors piste dans l'APPB | Violations |
|---|---|---|---|---|---|---|---|
| Le Laus → col d'Izoard | 5,75 km (6,57) | 620 m (617) | 4 h 58 (2 h 11) | 296 / 697 (80 / 1 048) | 655 / 0 (1 000 / 0) | — | 0 |
| Plaine du Bourget → col de Bousson | 2,75 km (2,43) | 295 m (285) | 1 h 50 (1 h 24) | 450 / 6 (392 / 2) | 41 / 0 (225 / 0) | — | 0 |
| Risoux S → N, février | 12,1 km (15,0) | 277 m (359) | 6 h 46 (6 h 37) | 1 189 / 888 (852 / 1 859) | 5 / 0 (146 / 0) | 0 | 0 |
| Même demande, août | 10,2 km (10,2) | 198 m (188) | 3 h 37 (3 h 33) | 1 154 / 549 (1 171 / 558) | 9 / 0 (70 / 0) | 732 (670) | 0 |

Le coût pondère « exposé » ×3 : ce n'est plus un temps sur ces cellules. Le Laus → Izoard quitte une partie de la route damée de l'Izoard (« exposé » sur 1 000 cellules) pour un trajet plus court en terrain moins exposé (655 cellules « exposé », 339 « peu exposé (modèle) ») ; aucun des quatre itinéraires ne traverse de « très exposé », avant comme après.

## Cas qui domine (C4) : 10 × 10 km à 5 m

Poste (Intel Core Ultra 7 265K), Cervières, fenêtre 2 000 × 2 000 (4 M cellules), extrémités aux coins SO et NE : A* 0,165 s (1,19 M cellules développées, 36 Mo de recherche), Dijkstra 0,178 s, destination inaccessible 0,256 s ; requête complète ≈ 0,22 s, pic mémoire 108 Mo. Chapelle-des-Bois : A* 0,306 s. La mesure sur le Pixel 8 (`bench … --power`) reste à faire, téléphone branché.

## Limites

- Pas de forêt pour l'instant ; exposition aux avalanches : modèle plus emprises CLPA, rien n'est « sûr ».
- `cost_s` n'est un temps que sur neige suffisante, hors piste, sans zone à éviter ni dévers ; le temps estimé est l'affaire de C5. Poids à calibrer.
- Itinéraires balisés = graphe OSM, approximation des annexes des arrêtés ; sens unique ignoré ; 8 voisins (longueur surestimée de 8 % au plus).
- Altitude : 8,6 Mo gzip à Cervières après le codage en différences (17,1 avant).
