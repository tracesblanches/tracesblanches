# skimap-srndiff

Difficulté d'un tracé de ski de randonnée nordique (SRN) : niveau (facile, moyen, difficile, très difficile), raisons classées et conseils, dans les deux sens de parcours. Ticket : SR1 (#134).

- [Échelles, seuils et règles, avec leurs sources](docs/methode.md)
- [Calibrage, accord avec les cotations publiées, relecture de 21 tracés, coût](docs/evaluation.md)

## Calcul (téléphone et serveur)

Aucune dépendance de pipeline : ni HTTP ni GeoTIFF (`cargo ndk -t arm64-v8a build -p skimap-srndiff`).

| Module | Rôle |
|---|---|
| `field` | lecture des couches en Lambert-93 : `Field` (raster), adaptateurs `SKR1` du hors piste (altitude en dm, pente en demi-degrés, classes d'exposition), `Grid<f32>` ; `Areas` (zones sans réseau, avec la zone analysée) ; `SiteZones` (règles C8 à une date) |
| `profile` | échantillons tous les 10 m dans l'ordre du tracé : altitude, pente, orientation, canopée (hauteur, couvert sur 15 m, trouée à 10 m), exposition, pistes, réseau, zones, sorties |
| `analysis` | mesures d'un sens : répartition des pentes, passages raides (montée, descente, dévers) et leur longueur, descente en forêt dense, plat, éloignement d'une sortie, réseau, avalanche, zones, altitude, orientation, temps de Munter |
| `rules` | niveau sur l'échelle SRN des clubs alpins (2025), raisons classées, conditions du jour facultatives (neige fraîche, croûte, regel) |
| `report` | conseils (peaux ou écailles, technique, glisse, passages à éviter, demi-tour, équipement, neige selon l'orientation) et texte français des deux sens |

```rust
use skimap_srndiff::{Layers, Params, assess, field, profile};
let elev = field::ElevationDm(&grid.elevation);      // OffpisteGrid (SKR1)
let slope = field::SlopeHalfDeg(&grid.slope);
let exp = field::Classes { grid: &exposure, nodata: 255 };
let layers = Layers { elevation: Some(&elev), slope: Some(&slope), exposure: Some(&exp), ..Layers::default() };
let p = profile::build(&track, &layers, Some(&road_points))?;
let report = assess(&p, &Params::default(), None);      // ou Some(&Conditions { icy: true, .. })
println!("{}", report.text_fr("Mon tracé"));
```

Règles lisibles : le niveau d'un sens est le plus haut des niveaux impliqués par la technique (pente moyenne sur 200 m, bornes calibrées 13,5 / 18,5 / 23,5° et 30°), l'effort (double des bornes de l'échelle 2025, raid) et des planchers ; jamais « facile » à travers du terrain que le modèle dit « exposé » ou « très exposé » ; jamais « sans danger » : le texte se termine toujours par « Indication calculée sur le relief et les cartes, jamais une garantie ». Une donnée absente (canopée, réseau, zones, exposition hors relief) est dite « non évaluée », jamais « absente ».

## Calibrage et évaluation (fonction `pipeline`)

```sh
cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- refs
cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- profiles --jobs 4   # puis roads (BD TOPO)
cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- evaluate
cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- review
cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- show "srn.com:la-vattay" [--fresh 30] [--icy]
cargo run --release -p skimap-srndiff --features pipeline --example srndiff -- bench datatourisme
cargo test -p skimap-srndiff --test real_sites -- --ignored   # SKR1 des deux sites, tracés C3
```

- `pipeline::refs` : tracés cotés (G5 : PNR du Vercors, OSM, Camptocamp ; skirandonneenordique.com, 49 topos français avec GPX) et tracés C3 ; groupes d'un même itinéraire entre sources.
- `pipeline::corridor` : relief et canopée LiDAR HD sur les seules dalles à moins de 1,5 km du tracé (téléchargeur commun, fonctions de `skimap-terrain` et `skimap-avalanche`), modèle d'exposition aux avalanches sur ce couloir, ou couche d'exposition du site (CLPA comprises) à Cervières et Chapelle-des-Bois ; réseau et zones des deux sites.
- `pipeline::roads` : routes ouvertes l'hiver de la BD TOPO de l'IGN (WFS) comme sorties.
- `pipeline::eval` : matrices de confusion, kappa pondéré, calibrage sur une grille, validation en laissant un itinéraire (toutes sources) de côté.

Sorties dans `data/out/france/srndiff/` : `references.json`, `profiles/<id>.json`, `evaluation.json`, `review.txt`, `srndiff.meta.json`. Les traces de skirandonneenordique.com (aucune licence affichée) ne servent qu'à l'évaluation : elles restent dans le cache et dans ces fichiers locaux, jamais dans un pack.
