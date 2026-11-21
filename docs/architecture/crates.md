# Crates et règle de propriété

[← Architecture](../architecture.md)

## Carte des crates

Toutes les crates sont dans `crates/`, nommées `skimap-<dossier>` (bibliothèque `skimap_<dossier>`).

| Crate | Rôle | Côté | Tickets |
|---|---|---|---|
| `common` | sites (registre de tous les domaines `site::Registry`, tables de données `crates/common/data/sites.json` et `status_sources.json`, types de site, tronçons), géométrie, grilles, projections, arborescence `data/`, configuration, erreurs, schéma `<couche>.meta.json` (`meta`), écrivains atomiques JSON / GeoJSON (`output`), comparaison des noms de pistes entre sources (`names`) ; téléchargeur (fonction `download`), GeoTIFF Float32 (fonction `geotiff`) | les deux | F1 #1, P1 #14 |
| `osm` | pistes nordiques OSM, domaines de fond | pipeline | P2 #15, P3 #16 |
| `terrain` | relief IGN, pente, orientation | pipeline | P5 #18 |
| `snow` | carte d'enneigement estimée, validation | pipeline | P6 #19, P7 #20 |
| `meteo` | API Météo-France : BERA, AROME | pipeline | P8 #21, P19 #32 |
| `zones` | zones protégées, règles de bivouac et d'accès | pipeline | P9 #22, P10 #23 |
| `coverage` | zones sans aucun réseau (ARCEP) | pipeline | P12 #25 |
| `refuges` | refuges et abris (refuges.info, OSM, Camptocamp), fiches pratiques filtrées des données personnelles, rafraîchissement planifié (`refresh`) ; voir `crates/refuges/README.md` | pipeline | P11 #24, H1 #132 |
| `stations` | fiches des 219 stations nordiques (secours, 112 en tête ; points de vente ; contacts ; parkings), titres de redevance, prix datés et zones de validité lus sur les pages des émetteurs (usage personnel), rafraîchissement planifié, `data/out/france/stations/` ; voir `crates/stations/README.md` | pipeline | H1 #132, H2 #133 |
| `srn` | domaines de jeu et traversées du ski de randonnée nordique hors stations damées, `data/out/france/srn/` ; voir `crates/srn/README.md` | pipeline | G5 #131 |
| `srndiff` | difficulté d'un tracé SRN dans les deux sens (facile à très difficile), raisons classées et conseils ; profils le long du tracé avec la fonction `pipeline` ; voir `crates/srndiff/README.md` | les deux | SR1 #134 |
| `pisteplan` | plan des pistes officiel transcrit sur nos géométries OSM (nom, couleur, départs, boucle), jamais redistribué ; lecteur de la transcription sans fonction, construction avec la fonction `pipeline` | les deux | P22 #121 |
| `status` | état des pistes ENJ | pipeline | P13 #26 |
| `avalanche` | exposition aux avalanches (oracle, Rust, validation) | pipeline | P16 #29, P17 #30, P18 #31 |
| `pack` | fond de carte, format de pack, visualiseur | les deux | P4 #17, P14 #27, P15 #28 |
| `core` | routage, rattachement, hors piste, temps estimé, demi-tour, zones, batterie (un module par ticket) | téléphone | C1 #33 à C10 #42 |
| `assist` | briefing, fartage, qualité de neige, affluence, suggestions, équipement, synthèse, alertes, meilleur moment, bilan, questions (un module par ticket) | téléphone | A1 #88 à A11 #98 |
| `voice` | banque de phrases, clips pré-générés | les deux | VO2 #84, VO3 #85 |
| `plan` | plan de journée (matin : boucle, heures de passage, demi-tour, annonces prévues, plans B), suivi sans manipulation (départ, pause, refuge, fin, écart, bascule sur un plan B, modes GPS), politique de parole, banc de scénarios ; voir `crates/plan/README.md` | téléphone | J1 #110 à J4 #113, Q9 #109, E1 #125 |
| `styles` | styles MapLibre par pratique (fond, SRN, guidage), générés et validés contre les tuiles et couches des packs | outil | K15 #123 |
| `ffi` | bibliothèque Android du noyau (UniFFI) : packs, styles instanciés sur les packs, synchro des packs, fiches FR / EN, catalogue des domaines, zones de redevance | téléphone | K1 #51, K2 #52, K15 #123, H2 #133, VO6 #129 |
| `server` | catalogue et paquets immuables des packs, mises à jour différentielles, minuterie de non-retour facultative, échange OAuth Strava ; voir `crates/server/README.md` et `deploy/README.md` | serveur | S0 #50, S1 #62, S2 #60 |
| `cli` | binaire `skimap` : une sous-commande par constructeur, `build-all` d'un site ou de toute la France (`batch.rs`), registre des sites et catalogue de la France (`france.rs`), rafraîchissement planifié (`refresh.rs`), seuil de relief de l'exposition (`relief.rs`) ; voir [compiler.md](compiler.md#ligne-de-commande) | outil | F1 #1, F4 #4, G2 #118, H1 #132, H2 #133 |
| `deps` | dépend de toutes les dépendances externes : `Cargo.lock` les fixe déjà, la CI prouve qu'elles compilent | outil | F1 #1 (orchestrateur) |

Les crates « téléphone » (`core`, `assist`, `voice`, `plan`, lecteur de `pack`) dépendent de `skimap-common` **sans** la fonction `download` : elles n'embarquent aucune pile HTTP (`cargo tree -p skimap-core` le montre). Les crates du pipeline l'activent : `skimap-common = { workspace = true, features = ["download"] }` (plus `"geotiff"` pour terrain et snow).

Dépendances entre crates du pipeline : `snow` → `terrain` (dalles et pente) ; `status` lit la couche `pistes` (géométrie des pistes du jeu de test alpin) ; `avalanche` → `meteo` (BERA du jour) ; `refuges` → `osm` (comparaison de noms) ; `stations` → `osm`, `refuges` (journal des changements, saison) et le recensement P3 sur disque ; `srn` → `osm` ; `coverage` → `zones` (partie française de l'emprise) ; `core` → `zones` (schéma). Les métadonnées et l'écriture des couches passent par `common`, pas par une autre crate.

## Règle de propriété

- Un agent ne modifie que la ou les crates de son ticket (dossier `crates/<crate>/`).
- `crates/cli` appartient à l'orchestrateur : l'agent expose une fonction publique testée dans sa crate (ex. `skimap_osm::extract(site, &layout)`), l'orchestrateur branche la sous-commande. Plusieurs agents qui éditeraient le même `match` entreraient en conflit.
- Dans `core` et `assist`, chaque ticket a son module (`routing.rs`, `eta.rs`, `wax.rs`…) : un agent ne touche que le sien ; `lib.rs` déclare déjà tous les modules.
- Dépendances : un agent n'ajoute des dépendances que dans le `Cargo.toml` de sa crate, avec `workspace = true`, en choisissant parmi `[workspace.dependencies]` du `Cargo.toml` racine.
- Une dépendance absente de cette liste, toute modification du `Cargo.toml` racine, de `crates/common`, de `crates/deps`, de `rust-toolchain.toml` ou de la CI passe par l'orchestrateur.
- Comme `crates/deps` utilise déjà toutes les dépendances, `Cargo.lock` les contient : ajouter `geo.workspace = true` à sa crate ne modifie que le bloc de sa crate dans `Cargo.lock`, sans conflit avec les autres agents.
- Le `Cargo.lock` est versionné ; ne pas le régénérer entièrement (`cargo update` est réservé à l'orchestrateur).

## Dépendances du workspace

Déclarées une fois dans `[workspace.dependencies]` du `Cargo.toml` racine :

- erreurs et journaux : `anyhow`, `thiserror`, `tracing`, `tracing-subscriber` ;
- formats : `serde`, `serde_json`, `quick-xml`, `csv`, `flate2`, `zip`, `sevenz-rust2` (archives ARCEP), `rusqlite` (GeoPackage, SQLite embarqué), `tiff` (MNT LiDAR HD), `pmtiles` (écriture) ;
- géométrie : `geo`, `geo-types`, `geojson`, `rstar`, `proj4rs`, `petgraph` ;
- temps : `jiff`, `httpdate` ; calcul : `rayon` ;
- HTTP : `reqwest` (bloquant, sans fonctions par défaut), `rustls` (ring), `webpki-roots`, `sha2`, `hex` ;
- ligne de commande : `clap` ; tests : `approx`, `tempfile`, `tiny_http` ;
- crates internes : `skimap-common`, `skimap-osm`, …, `skimap-plan`, `skimap-srn`, `skimap-stations` (chemins locaux ; `srndiff`, `pisteplan`, `styles`, `ffi` et `server` n'y figurent pas : aucune autre crate n'en dépend).

Copies corrigées de crates de crates.io dans `vendor/` (hors du workspace), branchées par `[patch.crates-io]` : `pmtiles` 0.28.1 (`leaf_offset` nul sans répertoire de feuilles, refusé par `pmtiles verify` ; voir `vendor/pmtiles/VENDOR.md`).
