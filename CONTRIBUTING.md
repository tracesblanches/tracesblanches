# Contribuer à Traces Blanches

[← README](README.md)

Les contributions sont bienvenues : correction, test, donnée, traduction, idée. Celles écrites avec un outil d'IA aussi, aux conditions de la section [Contributions écrites avec un outil d'IA](#contributions-écrites-avec-un-outil-dia).

- Un bogue, une idée, une donnée fausse ou un domaine à ajouter : ouvrez une issue avec le modèle qui correspond.
- Une faille de sécurité : ne l'ouvrez pas en public, suivez [SECURITY.md](SECURITY.md).
- Le [code de conduite](CODE_OF_CONDUCT.md) s'applique partout dans le projet.

Pour un changement important, ouvrez d'abord une issue, pour vérifier qu'il entre dans le projet avant d'y passer du temps.

## Licence des contributions

Le code de skimap est sous GPL-3.0-or-later avec une permission additionnelle pour les boutiques d'applications ([LICENSE-EXCEPTION](LICENSE-EXCEPTION)) ; `crates/avalanche` est sous EUPL-1.2. Cette permission ne couvre une contribution extérieure que si son auteur l'accorde expressément. Une demande de fusion n'est acceptée que si chaque commit porte une ligne `Signed-off-by` (`git commit -s`, [Developer Certificate of Origin 1.1](https://developercertificate.org/)) et si sa description contient la phrase :

> J'accepte que ma contribution soit diffusée sous GPL-3.0-or-later avec la permission additionnelle de LICENSE-EXCEPTION (sous EUPL-1.2 pour `crates/avalanche`), et j'en détiens les droits.

Une dépendance nouvelle doit passer `cargo deny --locked check` (licences permissives seulement, liste dans `deny.toml`) ; ensuite `docs/licences/third-party.sh` régénère [THIRD-PARTY.md](THIRD-PARTY.md).

## Contributions écrites avec un outil d'IA

Elles sont acceptées au même niveau d'exigence que les autres.

- **Relecture humaine.** Vous avez lu chaque ligne, vous la comprenez et vous savez l'expliquer en revue.
- **Preuves.** Tests, sorties de commandes collées et chiffres, comme pour toute contribution. « Ça devrait marcher » n'est pas une preuve.
- **Responsabilité.** Vous signez la contribution (`Signed-off-by`) et vous en répondez : exactitude, sécurité, droits sur le code. Vérifiez que l'outil n'a pas recopié du code sous une licence incompatible.
- **Taille raisonnable.** Une demande de fusion traite un sujet. Les lots générés et non relus sont refusés.

## Règles des données

- **Jamais de source à usage personnel dans un pack publié**, ni dans `fixtures/`, `benches/data/` ou `fuzz/`. Ces sources ne tournent qu'avec `SKIMAP_PERSONAL_SOURCES=1` ; `skimap_common::licence` décide, et l'étape `data-check` de `scripts/ci` vérifie les fichiers de données versionnés.
- **Jamais de plan des pistes de station redistribué** : ni image, ni couche raster, ni plan déformé, dans un pack ou dans le dépôt. Le plan peut être lu en local pour reporter ses informations sur nos propres tracés. Dans une issue, un lien vers le plan suffit.
- **OpenStreetMap (ODbL) et CC BY-SA (refuges.info, Camptocamp) ne se mélangent jamais** dans un même fichier ([docs/licences/l3-combinaison.md](docs/licences/l3-combinaison.md)).
- **Une nouvelle source arrive avec sa licence lue** : adresse, passage exact et date de lecture dans `docs/licences/`, attribution dans les métadonnées de la couche.
- **Corrections d'OpenStreetMap** : dans OpenStreetMap, selon ses règles. Aucun envoi automatique.

## Règles du code

- **Toute la France, aucun cas particulier** : aucun nom de site dans le code, les exemples ni la documentation des crates ; une valeur propre à un site va dans une table de données (`crates/*/data/*.json`, registre des sites). `crates/common/tests/no_site_names.rs` le vérifie. Les deux jeux de test ne sont que des données de test ; un document les appelle « jeu de test alpin » et « jeu de test jurassien » et écrit `<site>` dans ses commandes.
- **Tout est automatique** : aucun réglage obligatoire pour l'utilisateur, aucun nouveau bouton sur la carte ; une fonction se déclenche d'elle-même quand son contexte se présente.
- **Mots** : un texte affiché ne dit jamais « sans danger », « sûr » ni « safe » ; le modèle d'exposition aux avalanches décrit un terrain, il ne garantit rien. Une donnée qui n'a pas pu être rafraîchie garde sa date (« Mis à jour le… »).
- **Textes affichés** : en français et en anglais, toujours par les modules de textes (trait `Texts` côté Rust, `strings.xml` `values` et `values-fr` côté Android), jamais en dur.
- **Sources** : une source à usage personnel ou sans licence connue n'entre jamais dans un pack public (`skimap_common::licence`, option `SKIMAP_PERSONAL_SOURCES`). Chaque couche cite ses sources et leurs licences dans son `<couche>.meta.json`.
- **Écritures et verrous** : les fichiers produits s'écrivent par `skimap_common::output` (écriture atomique), les verrous par `skimap_common::output::FileLock` ; `clippy.toml` interdit les autres chemins.
- **Modèle d'exécution** : bloquant (`reqwest::blocking`, rayon), aucune API `async`. Python n'est jamais dans le chemin de calcul : seulement pour un oracle de référence ou un outil.
- **Mesures** : mesurer le cas qui domine le temps total, donner la commande et sa sortie, et la date ; une mesure sur les jeux de test vaut pour eux seuls.

## Tests

La CI tourne sur votre poste, pas sur GitHub Actions ([docs/ci-locale.md](docs/ci-locale.md)). Avant d'ouvrir une demande de fusion, lancez au moins `scripts/ci --quick` (fmt, clippy, tests) et collez sa sortie ; `scripts/ci` lance toutes les étapes. Les mêmes commandes, une à une :

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace                                # jeu de référence fixtures/, sans réseau
SKIMAP_DATA_DIR=<data complet> cargo test --release --workspace --locked --all-features -- --ignored full_data_
```

- Un défaut corrigé arrive avec un test qui échoue avant la correction et passe après.
- Les tests sur données complètes portent le préfixe `full_data_` et `#[ignore]` : ils lisent `SKIMAP_DATA_DIR` et n'y écrivent jamais.
- Côté Android : sous `android/`, `./gradlew testDebugUnitTest :app:verifyRoborazziDebug lint detekt ktlintCheck :app:assembleDebug` ([android/docs/verifier.md](android/docs/verifier.md)).

## Références dorées

`fixtures/` contient un jeu de données réduit des deux jeux de test, sources redistribuables seulement ([fixtures/LICENCES.md](fixtures/LICENCES.md)) ; `fixtures/golden/` les sorties de référence que les tests comparent (chiffres, phrases, empreintes).

- Une sortie qui change exprès : `SKIMAP_BLESS=1 cargo test -p <crate> --test <test>`, puis relire le diff de `fixtures/golden/` et l'expliquer dans le commit.
- Régénérer le jeu réduit depuis les données complètes : `cargo run --release -p skimap-cli -- fixtures`.

## Commits et demandes de fusion

- Messages en français, qui citent l'issue (`Refs #<n>`) ; un commit par changement cohérent, avec sa ligne `Signed-off-by`.
- Un sujet par demande de fusion. Remplissez le modèle : ce qui change, les preuves, la phrase d'accord.
- Textes publics et documentation : voix active, phrases courtes, faits vérifiés avec leur date, aucun superlatif.
