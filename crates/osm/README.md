# skimap-osm

Extraction des pistes nordiques OSM et recensement des domaines de fond (pipeline). Tickets : P2 (#15), P3 (#16).

- [P2 : pistes nordiques d'un site](docs/pistes.md) : sélection, attributs, réseau refermé hors de l'emprise, sens uniques, sorties, chiffres comparés aux rapports.
- [P2 : connexité du réseau et corrections OSM à faire](docs/connexite.md) : composantes et requêtes sans itinéraire, sens uniques à contresens à Chapelle-des-Bois, trou du Laus à Cervières.
- [P3 : domaines de fond, France](docs/domaines.md) : regroupement, recensement, sorties, résultats.

API : `extract::build(site, &layout, &downloader)` (P2), `census::build(site, &layout, &downloader)` (P3) ; `extract::load_planet` pour lire l'extrait une seule fois. Exemples : `cargo run --release -p skimap-osm --example pistes`, `--example domaines`.
