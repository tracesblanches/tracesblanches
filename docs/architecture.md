# Architecture du workspace Rust

La v1 est une preuve de concept entièrement en Rust : le pipeline de données et les algorithmes du noyau, évalués sur deux sites réels, Cervières (Hautes-Alpes) et Chapelle-des-Bois (Doubs). Le client Kotlin viendra ensuite (K0, #49). Python n'est jamais dans le chemin critique : il ne sert que d'oracle de référence ou d'outil (par exemple auto-ATES pour P16).

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
