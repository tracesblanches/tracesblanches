# Architecture du workspace Rust

Le pipeline de données et les algorithmes du noyau sont en Rust ; ils traitent tous les sites du registre (268 en France, [france.md](architecture/france.md)) de la même façon : tout ce qui est propre à un site vient d'une table de données, jamais du code (garde `crates/common/tests/no_site_names.rs`). Deux sites servent de jeux de test, mesures comprises ([donnees.md](architecture/donnees.md#jeux-de-test)). Le client est Kotlin (`android/`). Python n'est jamais dans le chemin critique : il ne sert que d'oracle de référence ou d'outil (par exemple auto-ATES pour P16).

En bref :

- un workspace cargo, édition 2024, résolveur 3, Rust 1.99.1 figé ; seize crates dans `crates/` ;
- un agent ne modifie que sa crate ; `common`, `cli`, `deps`, le `Cargo.toml` racine et la CI passent par l'orchestrateur ;
- tout est bloquant (`reqwest::blocking`, rayon), aucune API `async` ;
- données dans `data/` (ignoré par git) : `data/cache/<source>/`, `data/out/<site>/`, `data/out/france/` ;
- un seul schéma `<couche>.meta.json` (`common::meta`) pour toutes les couches.

## Sommaire

1. [Crates et règle de propriété](architecture/crates.md)
   - [Carte des crates](architecture/crates.md#carte-des-crates)
   - [Règle de propriété](architecture/crates.md#règle-de-propriété)
   - [Dépendances du workspace](architecture/crates.md#dépendances-du-workspace)
2. [Modèle d'exécution et téléchargeur commun](architecture/telechargeur.md)
   - [Modèle d'exécution : bloquant](architecture/telechargeur.md#modèle-dexécution--bloquant)
   - [Téléchargeur commun (P1, #14)](architecture/telechargeur.md#téléchargeur-commun-p1-14)
   - [Quotas par défaut](architecture/telechargeur.md#quotas-par-défaut)
3. [Données, configuration, sites et projections](architecture/donnees.md)
   - [Arborescence des données](architecture/donnees.md#arborescence-des-données)
   - [Configuration](architecture/donnees.md#configuration)
   - [Métadonnées des couches](architecture/donnees.md#métadonnées-des-couches)
   - [Sites](architecture/donnees.md#sites)
   - [Projections et grilles](architecture/donnees.md#projections-et-grilles)
4. [Compiler, tester, Android, git](architecture/compiler.md)
   - [Compiler et tester](architecture/compiler.md#compiler-et-tester)
   - [Ligne de commande](architecture/compiler.md#ligne-de-commande)
   - [Android (F4, #4)](architecture/compiler.md#android-f4-4)
   - [Git](architecture/compiler.md#git)
5. [Voix : banque de phrases, lecture par la synthèse du téléphone](voix.md)
   - [Banque de phrases (VO2)](voix/banque.md#banque-de-phrases-vo2)
   - [Lecture par le téléphone](voix/banque.md#lecture-par-le-téléphone-21112026)
