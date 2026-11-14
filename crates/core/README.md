# skimap-core

Noyau d'algorithmes partagé serveur / téléphone : routage, rattachement, hors piste, temps estimé, demi-tour, zones, batterie. Tickets : C1 (#33) à C10 (#42).

La crate ne contient aucune pile HTTP (`cargo tree -p skimap-core -e normal` : ni `reqwest`, ni `rustls`, ni `hyper`). La fonction `pipeline` (désactivée par défaut, côté pipeline seulement) ajoute la lecture des GeoTIFF par `skimap_common::geotiff` (MNT, neige ; `tiff`, sans réseau) : `routing::build` et `offpiste::prepare::prepare`.

## Modules

| Module | Ticket | Rôle |
|---|---|---|
| `graph` | C1, C2, C5 | graphe de pistes typé : nœuds, arêtes (sens unique, classique / skating, hors damage, usages, difficulté), profil d'altitude par arête, index R-tree des segments, rattachement au plus proche |
| `dem` (fonction `pipeline`) | C1 | lecture de `terrain/dem_5m.tif` (P5) et profil d'altitude de chaque arête tous les 10 m |
| `routing` | C1 | coûts enfichables (`CostModel`), préférences de style, Dijkstra entre deux points rattachés, sorties GeoJSON, `build(site, &layout)` |
| `eta` | C5 | modèle physique de temps, Munter, Tobler |
| `matching` | C2 | rattachement HMM (Viterbi en ligne), détection d'écart avec hystérésis, mode couloir hors piste, traces synthétiques |
| `ephemeris` | C7, C8 | coucher du soleil, crépuscule civil, heure légale (code tiré de `zone_eval`, partagé) |
| `turnaround` | C7 | heure limite de retour, heure de demi-tour, point de demi-tour d'un aller-retour |
| `offpiste` | C3, C4 | routage hors piste sur grille de coût de 5 m (rasters SKR1, politique de zones à une date, A*) ; `offpiste::prepare` (fonction `pipeline`) prépare les rasters |
| `zone_eval` | C8 | zones et règles à une position et une date |

## Pipeline et sorties

`skimap_core::routing::build(site, &layout)` (fonction `pipeline`) lit `data/out/<site>/pistes/graph_edges.geojson` et `graph_nodes.geojson` (P2) et `data/out/<site>/terrain/dem_5m.tif` (P5), et écrit `data/out/<site>/routes/` :

| Fichier | Contenu |
|---|---|
| `piste_graph.json` | graphe typé (`GraphData`, JSON serde) avec les profils d'altitude : c'est ce que le téléphone charge (`PisteGraph::from_json`), il ne lit jamais le MNT |
| `routes.geojson` | itinéraires de preuve (exemple `routes`) |
| `matching_example.geojson` | une trace synthétique, ses rattachements et ses alertes (exemple `matching_bench`) |
| `offpiste_*.geojson` | itinéraires hors piste de preuve (C3, exemple `offpiste_bench proofs`) |
| `routes.meta.json` | sources (OSM ODbL, IGN LiDAR HD Licence Ouverte), date de génération, date de validité des données OSM, fichiers et propriétés, paramètres, chiffres du graphe |

```sh
cargo run --release -p skimap-core --features pipeline --example routes          # C1, C5, C7
cargo run --release -p skimap-core --features pipeline --example matching_bench  # C2
cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- proofs  # C3
cargo test -p skimap-core --features pipeline
cargo test -p skimap-core --test real_sites -- --ignored  # données réelles
```

## Détail par ticket

- [C1 : graphe et routage](docs/routage.md) : sens unique, coût, style, chiffres et itinéraires de preuve, limites des données P2
- [C5 : temps estimé](docs/temps.md) : modèle physique, paramètres et sources, comparaison à Munter, Tobler, Skitourenguru
- [C3, C4 : routage hors piste](docs/horspiste.md) : rasters préparés, zones à une date, coût, preuves, cas qui domine
- [C2 : rattachement et écart](docs/rattachement.md) : HMM, hystérésis, mode couloir, traces synthétiques et mesures
- [C7 : demi-tour](docs/demi-tour.md) : limite de lumière, marge, heure et point de demi-tour
