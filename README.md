# skimap

Application Android de ski de fond et de ski de randonnée nordique, utilisable hors ligne. Elle couvre toute la France : les 268 sites du registre (domaines nordiques du recensement, zones et traversées SRN, `skimap sites`), tous construits par le même pipeline Rust, sans cas particulier par site. Deux d'entre eux servent de jeux de test (données de référence des tests, `fixtures/`) : un domaine alpin et un domaine jurassien ([données](docs/architecture/donnees.md#jeux-de-test)).

- `crates/` : workspace Rust (voir `docs/architecture.md`).
- `research/` : état des lieux, liste des fonctions envisagées et rapports de recherche.
- `docs/tickets.yaml` : découpage du travail en tickets, source des issues GitHub.

Statut : usage personnel, dépôt privé pour l'instant, licence à décider.

## Contribuer : CI locale

La CI tourne **sur le poste, jamais sur GitHub Actions** (décision du 21/11/2026 : aucun runner payant). Le dépôt ne contient aucun workflow GitHub (`.github/workflows/` supprimé) : une poussée ne déclenche aucune exécution, donc aucune facturation.

```sh
scripts/ci                 # tout : fmt, clippy, tests, scénarios, doc, cargo deny, données
                           # publiables, shellcheck, propriétés, fuzz, bancs, Android
scripts/ci --quick         # fmt, clippy -D warnings, cargo test --workspace (le hook pre-push)
scripts/ci --weekly        # tout, puis le build-all réel des sites de test (téléchargements
                           # compris) et les tests full_data_ sur ce qu'il a construit
scripts/ci --only full-data  # les tests full_data_ sur data/ tel qu'il est
scripts/ci --only fuzz,bench --fuzz-secs 600
scripts/ci --list          # les étapes et ce qu'elles lancent
```

- Résumé lisible à la fin (étape, état, durée, tests réussis ou échoués) et code de retour 0 seulement si tout passe ; un journal par étape dans `~/.cache/skimap-ci/<date>/` (`latest` pointe sur le dernier).
- Données complètes : `SKIMAP_DATA_DIR`, sinon `data/` du dépôt ou du dépôt principal (worktrees) ; les tests de `skimap-plan` (scénarios Q9, allocations, allure) les lisent. Les tests `full_data_` comparent le code aux sorties de `build-all` : ils tournent dans la tâche hebdomadaire, qui reconstruit avec le code courant dans `~/.cache/skimap-ci/weekly/data` (jamais dans `data/`, qui peut dater d'un code plus ancien).
- Bancs : instructions comptées sur la base (`--bench-base`, par défaut la base de fusion avec `main`) et sur l'arbre de travail, échec au-delà de +5 % (`--bench-limit`).
- Outils épinglés installés dans `~/.cargo/bin` s'ils manquent : cargo-deny 0.20.2, cargo-fuzz 0.13.2, cargo-ndk 4.1.2, gungraun-runner 0.20.0 ; il faut aussi valgrind, shellcheck, le JDK 21, le SDK et le NDK Android (`android/README.md`).
- Aucun test sur un appareil : les tests instrumentés Android restent manuels ([android/docs/verifier.md](android/docs/verifier.md)).

Hook `pre-push` versionné (`.githooks/pre-push`), à activer une fois par clone :

```sh
git config core.hooksPath .githooks
```

Il lance `scripts/ci --quick` avant chaque poussée et la refuse si une étape échoue ; `SKIMAP_PREPUSH=full git push` lance toute la CI, `SKIMAP_PREPUSH=fuzz,bench git push` ces étapes. Il vérifie l'arbre de travail tel qu'il est et ne touche pas à la configuration git (la signature des commits reste active).
