# pmtiles 0.28.1, copie corrigée

Copie de la crate `pmtiles` 0.28.1 de crates.io (sources identiques au commit `425c1d10b61eba9481ea6713b64afa27cd1fce3c` de `stadiamaps/pmtiles-rs`, vérifié par `diff -r`), branchée par `[patch.crates-io]` dans le `Cargo.toml` racine. Licences MIT / Apache-2.0 (fichiers `LICENSE-*`). Exclue du workspace.

## Différences avec l'original

Deux corrections.

1. Lecture d'un fichier (`src/sync_reader.rs`, `impl SyncBackend for File`), trouvée par le fuzzing du lecteur du téléphone (Q3, #103) : le tampon était alloué à la longueur demandée (`vec![0; length]`), longueur lue dans l'en-tête ou les répertoires de l'archive. Un fichier corrompu de quelques centaines d'octets demandant 3 Go de métadonnées faisait allouer 3 Go (le processus est tué). Le tampon est maintenant borné à ce que le fichier contient après `offset` (un `fstat` par lecture) ; une lecture plus courte que demandée donne l'erreur habituelle du lecteur. Entrée de régression : `fuzz/regressions/tile_archive/metadata-3gb` (3 000 000 000 octets demandés avant la correction, `ERROR: libFuzzer: out-of-memory (malloc(3000000000))`). Le lecteur en mémoire (`impl SyncBackend for Bytes`) bornait déjà la tranche.

2. Écriture, dans `src/writer/mod.rs` (`build_directories`) : sans répertoire de feuilles, l'écrivain laissait `leaf_offset = 0` dans l'en-tête, que `pmtiles verify` (go-pmtiles 1.31.2) refuse (« Leaf directories offset=0 must not be 0 »). L'offset pointe maintenant, comme dans go-pmtiles, sur la fin des données (longueur 0). Test ajouté : `writer::tests::leaf_offset_without_leaf_directories` (échoue sans la correction : `left: 0, right: 16466`).

Les tests de la crate ne tournent pas dans le workspace (il leur faut les `fixtures/` du dépôt amont et les dépendances de développement) : `diff -u` avec l'original et les 19 tests `writer::` passés sur une copie avec les fixtures du commit amont sont dans le rapport de l'intégration du 15/11/2026.

À retirer quand une version publiée contient les deux corrections (texte d’issue / PR préparé pour la seconde, non publié ; la première est à signaler aussi).
