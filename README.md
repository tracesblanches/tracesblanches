# skimap

Application Android de ski de fond et de ski de randonnée nordique, utilisable hors ligne. Elle couvre toute la France : les 268 sites du registre (domaines nordiques du recensement, zones et traversées SRN, `skimap sites`), tous construits par le même pipeline Rust, sans cas particulier par site. Deux d'entre eux servent de jeux de test (données de référence des tests, `fixtures/`) : un domaine alpin et un domaine jurassien ([données](docs/architecture/donnees.md#jeux-de-test)).

- `crates/` : workspace Rust (voir `docs/architecture.md`).
- `research/` : état des lieux, liste des fonctions envisagées et rapports de recherche.
- `docs/tickets.yaml` : découpage du travail en tickets, source des issues GitHub.

Statut : usage personnel, dépôt privé pour l'instant, licence à décider.
