# CI locale

Retour au [README](../README.md).

La CI tourne **sur le poste, jamais sur GitHub Actions** (décision du 21/11/2026 : aucun runner payant). Le dépôt ne contient aucun workflow GitHub (`.github/workflows/` supprimé) : une poussée ne déclenche aucune exécution, donc aucune facturation.

```sh
scripts/ci                 # tout : fmt, clippy, tests, scénarios, doc, cargo deny, données
                           # publiables, shellcheck, propriétés, fuzz, bancs, Android
scripts/ci --quick         # fmt, clippy -D warnings, cargo test --workspace (exigé par le hook pre-push)
scripts/ci --weekly        # tout, puis le build-all réel des sites de test (téléchargements
                           # compris) et les tests full_data_ sur ce qu'il a construit
scripts/ci --only full-data  # les tests full_data_ sur data/ tel qu'il est
scripts/ci --only fuzz,bench --fuzz-secs 600
scripts/ci --list          # les étapes et ce qu'elles lancent
scripts/ci --stages quick  # les étapes exigées par le hook (quick ou full), une par ligne
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

Le hook **ne lance pas la CI** : git l'exécute alors que la connexion SSH vers GitHub est déjà ouverte, et GitHub la coupe au bout de quelques minutes d'inactivité (« Connection to github.com closed by remote host ») ; une CI de 11 min dans le hook faisait donc échouer la poussée. La CI se lance avant, et le hook vérifie son résultat :

```sh
git status                 # rien à valider, rien de non suivi
scripts/ci --quick         # sur le commit à pousser
git push origin main       # accepté aussitôt
```

- Résultats par arbre : une exécution de `scripts/ci` sur une copie **propre** (`git status --porcelain` vide, fichiers non suivis compris, le même arbre à la fin qu'au début) ajoute chaque étape réussie à `~/.cache/skimap-ci/validated/<sha de l'arbre>` (ligne `<étape> <commit> <date> <journaux>` ; `SKIMAP_CI_LOG_ROOT` déplace le dossier). Une exécution sur un arbre modifié n'est pas retenue. Le sha de l'arbre ne dépend que du contenu : un commit refait avec le même contenu (message corrigé, fusion sans changement) reste validé, et les worktrees partagent ces résultats.
- Le hook accepte la poussée si l'arbre de chaque commit poussé a les étapes de `scripts/ci --quick` (fmt, clippy, test ; une CI complète les contient) ; `SKIMAP_PREPUSH=full git push` exige toutes les étapes (`scripts/ci --stages full`), `SKIMAP_PREPUSH=fuzz,bench git push` celles-là. Sinon il refuse en disant quelles étapes manquent et quoi lancer. Une suppression de branche passe sans vérification.
- Il ne touche pas à la configuration git : la signature des commits reste active.
