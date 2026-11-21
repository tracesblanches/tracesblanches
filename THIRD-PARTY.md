# Composants tiers et leurs licences

Généré par `docs/licences/third-party.sh` à partir de `Cargo.lock` et de la résolution
Gradle de l'app, avec les tables de `docs/licences/tiers/` ; ne pas l'éditer à la main.
`docs/licences/third-party.sh --check` échoue si l'un de ces fichiers n'est plus à jour.

La licence du code de skimap est décrite dans le [README](README.md#licence) ; la licence
des données produites (packs, couches), par source, dans
[docs/licences-donnees.md](docs/licences-donnees.md).

## Code et modèles repris hors dépendances

| Composant | Licence | Ce que le dépôt en fait |
|---|---|---|
| com4FlowPy (AvaFrame), commit `d0451ce` | EUPL-1.2 | `crates/avalanche/src/exposure/flowpy.rs` en est un portage Rust ; la crate `skimap-avalanche` reste sous EUPL-1.2 ([LICENSES/EUPL-1.2.txt](LICENSES/EUPL-1.2.txt)) et n'est liée que par le pipeline, jamais par l'app |
| AutoATES v3.0, « Copyright (c) 2024 AutoATES » | MIT | zones de départ, classes et paramètres portés dans `crates/avalanche` ; l'oracle Python est hors du chemin de calcul |
| pmtiles 0.28.1 (`vendor/pmtiles`) | MIT OR Apache-2.0 | copie corrigée, utilisée sous MIT ; textes dans `vendor/pmtiles/LICENSE-MIT` et `LICENSE-APACHE`, modification décrite dans `vendor/pmtiles/VENDOR.md` |
| Protomaps basemaps | BSD-3-Clause | style de départ de `crates/pack/assets/protomaps/style-light-fr.json` |
| snowtools (CNRM) | CeCILL-C v1 | aucun fichier dans le dépôt ; contours des massifs lus par le pipeline, cités dans la couche qui en dérive |
| Pocket TTS (Kyutai), modèle | CC-BY-4.0 et conditions d'usage du modèle | génère les clips de voix sur le poste ; non livré dans l'app |
| pockettts.cpp, ggml | MIT | outil du poste pour la voix ; non livré dans l'app |

## Crates Rust

383 crates tierces dans `Cargo.lock` (toutes fonctions, cibles Linux et Android), dont 124 liées dans la bibliothèque de l'app (`skimap-ffi`, colonne « App ») ; licence inconnue : 0. Une licence « A OR B » laisse le choix.

- [Crates Rust (1)](docs/licences/tiers/crates-1.md) : lignes 1 à 85
- [Crates Rust (2)](docs/licences/tiers/crates-2.md) : lignes 86 à 170
- [Crates Rust (3)](docs/licences/tiers/crates-3.md) : lignes 171 à 255
- [Crates Rust (4)](docs/licences/tiers/crates-4.md) : lignes 256 à 340
- [Crates Rust (5)](docs/licences/tiers/crates-5.md) : lignes 341 à 383

## Bibliothèques Android

169 bibliothèques dans le classpath d'exécution de la version release de l'app (`:app`, `releaseRuntimeClasspath`), licence lue dans leur POM (plusieurs licences séparées par « ; » : toutes celles que le POM déclare) ; licence absente du POM : 0.

- [Bibliothèques Android (1)](docs/licences/tiers/android-1.md) : lignes 1 à 85
- [Bibliothèques Android (2)](docs/licences/tiers/android-2.md) : lignes 86 à 169
