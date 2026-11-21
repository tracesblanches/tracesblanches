# Preuves chiffrées

[← README](../README.md)

Chaque chiffre porte sa commande et sa date ; les mesures sur données réelles valent pour les deux jeux de test ([définition](architecture/donnees.md#jeux-de-test)), pas pour toute la France.

## Tests

Mesuré le 21/11/2026 sur `0ea8b76` plus les champs `license` des manifestes (code inchangé), cargo-llvm-cov 0.9.1, Rust 1.99.1 :

```sh
SKIMAP_DATA_DIR=<data complet> cargo llvm-cov --workspace --locked --json --summary-only --output-path cov.json
```

1 177 tests passés, 0 en échec, 71 ignorés (`#[ignore]`, dont les tests `full_data_`), dans 104 binaires de test ; fonctions par défaut des crates. Couverture des lignes des fichiers `src/` de chaque crate (les modules `#[cfg(test)]` de ces fichiers comptent, limite de llvm-cov) ; la colonne Q4 rappelle la mesure du ticket #104, faite avant les crates arrivées depuis.

| Crate | Lignes couvertes | Lignes | Couverture | Q4 (#104) |
|---|---:|---:|---:|---:|
| `assist` | 9 873 | 10 186 | 96,9 % | 78,9 % |
| `avalanche` | 3 491 | 4 849 | 72,0 % | 63,4 % |
| `cli` | 3 529 | 7 325 | 48,2 % | 26,2 % |
| `collab` | 1 742 | 1 866 | 93,4 % | |
| `common` | 3 588 | 3 853 | 93,1 % | 91,9 % |
| `core` | 6 616 | 6 849 | 96,6 % | 96,0 % |
| `coverage` | 635 | 902 | 70,4 % | 71,7 % |
| `deps` | 32 | 32 | 100,0 % | |
| `ffi` | 7 374 | 10 008 | 73,7 % | |
| `group` | 808 | 840 | 96,2 % | |
| `intent` | 406 | 414 | 98,1 % | |
| `meteo` | 1 379 | 2 002 | 68,9 % | 70,9 % |
| `osm` | 4 103 | 5 469 | 75,0 % | 79,1 % |
| `pack` | 9 683 | 11 941 | 81,1 % | 77,4 % |
| `pisteplan` | 927 | 1 091 | 85,0 % | 93,4 % |
| `plan` | 8 145 | 8 579 | 94,9 % | 92,2 % |
| `refuges` | 1 620 | 2 281 | 71,0 % | 62,2 % |
| `server` | 2 702 | 3 153 | 85,7 % | 85,6 % |
| `snow` | 757 | 1 409 | 53,7 % | 54,0 % |
| `srn` | 1 425 | 2 546 | 56,0 % | |
| `srndiff` | 2 365 | 2 505 | 94,4 % | |
| `stations` | 3 125 | 5 168 | 60,5 % | |
| `status` | 1 565 | 2 149 | 72,8 % | 70,6 % |
| `styles` | 2 739 | 3 326 | 82,4 % | 76,9 % |
| `terrain` | 631 | 947 | 66,6 % | 67,4 % |
| `voice` | 3 376 | 4 121 | 81,9 % | 78,6 % |
| `zones` | 1 393 | 2 196 | 63,4 % | 57,3 % |
| **workspace** | **84 029** | **106 007** | **79,3 %** | 76,8 % |

Durée des tests instrumentés : 40 min 44 s (binaires déjà compilés), poste chargé par d'autres compilations (charge moyenne 100 à 150 sur 20 cœurs) ; `plan --test english` (`the_day_plan_in_english`) a tourné à lui seul plus de 12 min.

## Mutation

`cargo mutants -p <crate>` (tests du paquet : bibliothèque et tests d'intégration ; `--features pipeline` pour `core`). Score = (tués + délais) / (tués + survivants + délais). Mesuré lors du ticket Q4 (#104), sur 20 cœurs avec `-j 12` ; les survivants restants sont justifiés dans l'issue (calculs sous la précision des formules, synthèse vocale testée seulement avec le modèle, assistant dépendant de la météo horaire).

| Crate | Mutants | Score avant Q4 | Score après Q4 | Durée |
|---|---:|---:|---:|---:|
| `core` | 2 367 | 64,9 % | 82,9 % (377 survivants) | ≈ 20 min |
| `voice` | 908 | 69,3 % | 79,3 % (177 survivants) | ≈ 6 min |
| `assist` | 2 084 | 48,4 % | 49,8 % (937 survivants) | ≈ 22 min |

## Construction d'un site

`build-all`, cache rempli, avec les sources à usage personnel (15/11/2026, 21 étapes) : 99,3 s sur le jeu de test alpin, dont 81,4 s pour l'exposition aux avalanches, l'étape qui domine ; 22,0 s sur le jeu de test jurassien. Quand le cache du recensement a expiré, sa relecture (1 requête par minute) prend 493 s et domine à son tour. Détail : [compiler.md](architecture/compiler.md#ligne-de-commande).

