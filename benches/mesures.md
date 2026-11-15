# Bancs des cas dominants (Q5, #105)

Deux niveaux :

- **comptage d'instructions** (Callgrind par gungraun 0.20, déterministe : deux exécutions du même binaire donnent les mêmes nombres) sur des entrées réelles réduites versionnées dans `benches/data` ; c'est la vérification de non-régression de la CI (`benches/compare.sh`, job `bench` de `.github/workflows/ci.yml`) ;
- **temps réel sur le poste** sur les entrées complètes de `data/out` : banc `wall` de skimap-core, exemples `offpiste_bench` et `matching_bench`, `skimap build-all`.

## Cas mesurés

| Cas | Comptage (CI) | Temps réel (poste) |
|---|---|---|
| Hors piste, plus grande grille | `dominant/offpiste_route` : Le Laus → col d'Izoard sur l'extrait de la grille de Cervières (407 × 1024 cellules de 5 m, 2 × 5 km ; neige du 15/02/2026, exposition du modèle seul) ; `offpiste_explore` : recherche exhaustive de la même fenêtre | `wall` sur la grille entière (3317 × 3480) ; `offpiste_bench bench cervieres … --window-km 20` (fenêtre de 11 M cellules, A* de 14,8 km) |
| Rattachement d'une sortie entière | `dominant/matching_outing` : Le Laus → refuge des Fonts et retour, 7 576 fixes (1 / s, bruit 10 m, forêt) | `wall` ; `matching_bench` (464 475 et 533 961 fixes) |
| Routage sur le graphe complet | `dominant/routing_full_graph` : précalcul et 40 itinéraires sur tout le graphe de Chapelle-des-Bois (773 arêtes) | `wall` |
| Exposition aux avalanches de Cervières | `exposure/exposure_model` : modèle complet sur l'extrait de 64 × 64 cellules le plus exposé (rayon à un fil, le fil du banc) | étape `avalanche-exposure` de `build-all cervieres` |
| build-all complet | — (téléchargements, 5 Go de cache) | `skimap build-all` sur les deux sites ; chaque semaine en CI (`weekly.yml`) |

## Comptages (poste, Debian 13, valgrind 3.24, 15/11/2026)

```text
dominant::offpiste::offpiste_route laus_izoard        Instructions: 246 847 241
dominant::offpiste::offpiste_explore whole_crop       Instructions: 236 860 251
dominant::trails::matching_outing laus_fonts_and_back Instructions: 171 379 280
dominant::trails::routing_full_graph chapelle_40_pairs Instructions:  50 037 092
exposure::exposure::exposure_model cervieres_steepest Instructions: 2 807 607 832
```

Mêmes sources construites et mesurées sous Ubuntu 24.04 (valgrind 3.22, conteneur Docker réglé comme un runner : `--ulimit nofile=65536`, seccomp sans filtre pour `setarch`) : 246 287 023, 236 825 327, 169 491 120, 51 247 679 et 2 871 626 132 ; écarts de 0,1 à 2,3 % (glibc, version de valgrind) : les nombres ne se comparent que sur la même machine, d'où la comparaison base / tête de `compare.sh` plutôt que des valeurs stockées. Entre la base et la tête construites dans deux répertoires, écarts mesurés ≤ 0,034 %. Valgrind 3.26 (Ubuntu 26.04) fonctionne aussi.

## Preuve : un ralentissement de 10 % fait échouer la vérification

Boucle ajoutée à chaque expansion de la recherche A* (`offpiste::Router::search`), retirée ensuite :

```text
$ benches/compare.sh HEAD          # base = HEAD, tête = arbre de travail ralenti
dominant::offpiste::offpiste_route laus_izoard:(cases :: setup_offpiste())
  Instructions:                                  272027378|246847271            (+10.2007%) [+1.10201x]
Performance has regressed: Instructions (246847271 -> 272027378) regressed by +10.2007% (>+5.00000%)
dominant::offpiste::offpiste_explore whole_crop:(cases :: setup_offpiste())
  Instructions:                                  264213540|236860249            (+11.5483%) [+1.11548x]
Performance has regressed: Instructions (236860249 -> 264213540) regressed by +11.5483% (>+5.00000%)
dominant::trails::matching_outing laus_fonts_and_back:(cases :: setup_outing())
  Instructions:                                  166703928|166760629            (-0.03400%) [-1.00034x]
dominant::trails::routing_full_graph chapelle_40_pairs:(cases :: setup_network())
  Instructions:                                   50037092|50037224             (-0.00026%) [-1.00000x]
    Callgrind: Instructions (246847271 -> 272027378): +10.2007% exceeds limit of +5.00000%
    Callgrind: Instructions (236860249 -> 264213540): +11.5483% exceeds limit of +5.00000%
Gungraun result: Regressed. 2 without regressions; 2 regressed; 0 filtered; 4 benchmarks finished in 6.43969s
EXIT 3
```

Sans la boucle, même commande : « Gungraun result: Ok », code 0.

## Temps réel sur le poste (20 fils, 15/11/2026)

Le poste était partagé avec d'autres calculs (charge moyenne sur une minute indiquée) : les temps sont des majorants.

Banc `wall` (charge 7) :

```text
offpiste_route (crop, Laus → Izoard)         median     22.97 ms   min     22.34 ms   (7 runs)
offpiste_explore (crop, exhaustive)          median     22.94 ms   min     22.71 ms   (7 runs)
matching_outing (7576 fixes)                 median      9.87 ms   min      9.77 ms   (7 runs)
routing_full_graph (Chapelle, 40 routes)     median      3.97 ms   min      3.91 ms   (7 runs)
full grid: 3317 × 3480 cells
offpiste_route (whole grid, Laus → Izoard)   median     42.45 ms   min     41.77 ms   (7 runs)
offpiste_explore (whole grid, exhaustive)    median    473.43 ms   min    470.20 ms   (3 runs)
```

`offpiste_bench bench cervieres snow_2026-02-15.skr --window-km 20 --iters 3` (charge 29) : fenêtre de 11 002 489 cellules, A* de 14,8 km à vol d'oiseau (37,7 km de chemin) en 0,335 s (médiane ; 2 280 932 cellules développées), Dijkstra 0,349 s, recherche exhaustive 0,470 s.

`matching_bench` (σ 15 m + forêt, tous scénarios) : 2,5 µs par fix en moyenne à Cervières, 1,1 µs à Chapelle-des-Bois (p99 6,2 et 3,1 µs) ; une sortie de 3 h à 1 fix / s : 27 ms.

`skimap build-all` avec le cache rempli, sans `SKIMAP_PERSONAL_SOURCES` (comme le job hebdomadaire ; sorties dans un répertoire de travail, cache en liens symboliques vers `data/cache`) :

| Site | Total | Étape dominante | Charge |
|---|---|---|---|
| Cervières | 122,9 s (CPU 1 571 s) | `avalanche-exposure` 105,9 s (86 %) | 15 à 34 |
| Chapelle-des-Bois | 46,6 s (CPU 96 s) | `avalanche-exposure` 17,5 s | 31 |

Référence au repos avec `SKIMAP_PERSONAL_SOURCES=1` (`docs/architecture/compiler.md`) : 99,3 s dont 81,4 s d'exposition à Cervières, 22,0 s à Chapelle-des-Bois. Avec les sources personnelles et un cache du widget Nordic France expiré, `domaines` attend 1 requête par minute (493 s) : c'est alors l'étape dominante.

## Commandes

```sh
cargo install gungraun-runner --version 0.20.0 --locked      # et valgrind
cargo bench -p skimap-core --bench dominant                   # comptages
cargo bench -p skimap-avalanche --bench exposure
benches/compare.sh <révision de base> [5%]                    # vérification, code 3 si régression
cargo bench -p skimap-core --bench wall                       # temps réel
SKIMAP_MAKE_BENCH_DATA=1 cargo bench -p skimap-core --features pipeline --bench make_data   # régénère benches/data
```

Les entrées de `benches/data` (996 Ko) viennent de `data/out`, sources publiables seulement : exposition du modèle seul (sans la fusion CLPA de la couche), table des classes de zones sans les identifiants de sources à licence non vérifiée ; sources et licences dans `bench_data.meta.json`, vérification `.github/scripts/check-unpublishable-data.sh` (job `workflows`) ; elles seront à remplacer par le jeu réduit versionné de Q1 (#101, `fixtures/`) quand il sera fusionné.
