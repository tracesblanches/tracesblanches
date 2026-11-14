# skimap-cli

Binaire `skimap` (clap) : sous-commandes du pipeline et des algorithmes, compilé aussi pour Android arm64. Tickets : F1 (#1), F4 (#4) ; réservé à l'orchestrateur, qui branche chaque sous-commande sur la fonction exposée par la crate du ticket.

Sous-commandes, `build-all` et exemples : [docs/architecture/compiler.md](../../docs/architecture/compiler.md#ligne-de-commande). Les constructeurs sont dans `src/build.rs`.
