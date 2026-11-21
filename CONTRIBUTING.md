# Contribuer à skimap

[← README](README.md)

## Licence des contributions

Le code de skimap est sous GPL-3.0-or-later avec une permission additionnelle pour les boutiques d'applications ([LICENSE-EXCEPTION](LICENSE-EXCEPTION)) ; `crates/avalanche` est sous EUPL-1.2. Cette permission ne couvre une contribution extérieure que si son auteur l'accorde expressément. Une demande de fusion n'est acceptée que si sa description contient la phrase :

> J'accepte que ma contribution soit diffusée sous GPL-3.0-or-later avec la permission additionnelle de LICENSE-EXCEPTION (sous EUPL-1.2 pour `crates/avalanche`), et j'en détiens les droits.

Une dépendance nouvelle doit passer `cargo deny --locked check` (licences permissives seulement, liste dans `deny.toml`) ; ensuite `docs/licences/third-party.sh` régénère [THIRD-PARTY.md](THIRD-PARTY.md).

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

## Commits

Messages en français, qui citent le ticket (`Refs #<n>`) ; un commit par changement cohérent. Les tickets sont décrits dans `docs/tickets.yaml`.
