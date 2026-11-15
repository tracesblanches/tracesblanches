# Compiler, tester, Android, git

[← Architecture](../architecture.md)

## Compiler et tester

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p skimap-common          # sans la fonction download (côté téléphone)
cargo clippy -p skimap-core --features pipeline --all-targets -- -D warnings
cargo test -p skimap-core --features pipeline   # côté pipeline du noyau (GeoTIFF, exemples)
cargo clippy -p skimap-pack --features pipeline --all-targets -- -D warnings
cargo test -p skimap-pack --features pipeline   # constructeur du pack, export
cargo run -p skimap-cli -- sites
cargo run -p skimap-cli -- fetch-url 'https://geo.api.gouv.fr/communes/05027' --source geo-api
cargo run --release -p skimap-cli -- build-all cervieres
```

## Ligne de commande

Une sous-commande par constructeur de couche, qui appelle la fonction publique de la crate : `pistes`, `domaines` (France puis les sites), `terrain`, `snow [--date]`, `snow-validate [--seasons A B]`, `refuges`, `zones`, `coverage [--release YYYY_TQ] [--archives DIR]`, `bera [--date]`, `meteo`, `status-poll`, `avalanche-historic [--dump FICHIER]`, `avalanche-exposure` (exposition aux avalanches P16 à P18, après `terrain` et `pistes`, télécharge une fois la hauteur de canopée LiDAR HD), `routing` (graphe de pistes avec profils d'altitude, C1), `offpiste-prepare [--snow-dir DIR]...` (rasters du hors piste, C3 ; par défaut la couche `snow` du site ; avec l'exposition aux avalanches de la couche `avalanche-exposure` si elle existe, surcouche par défaut du routeur), `voice` (clips de la voix, VO3, noms des couches `pistes`, `refuges` et `zones`, Pocket TTS sur le GPU du poste : 19,6 s à Cervières, 26,5 s à Chapelle-des-Bois), et deux sous-commandes du noyau : `zones-at <site> <lat> <lon> <date>` (C8 sur la couche `zones`) et `route <site> <lat,lon> <lat,lon> [--style classic|skating] [--date YYYY-MM-DD] [--start HH:MM] [--max-snap M] [--out FICHIER]` (itinéraire le plus rapide sur pistes, C1 ; longueur, D+ / D−, temps estimé C5 ; avec `--start`, limite de retour avant la nuit, arrivée et demi-tour d'un aller-retour, C7 ; GeoJSON, `route.geojson` par défaut ; lit `routes/piste_graph.json` comme le téléphone). `assist <site> [--date] [--days N] [--home lon,lat] [--route FICHIER] [--suggest] [--outings] [--history past|with-forecast]` (assistant A1 à A9, couche `assist`), `history <site> <premier> <dernier> [--no-snow] [--no-bera]` (météo des jours passés, usage personnel, cartes d'enneigement et BERA archivés pour l'assistant), `build-pack <site>` (pack hors ligne, P14, relu par le lecteur du téléphone), `export <site>` (Locus Map et OsmAnd, P21), `serve [RACINE] [--addr 127.0.0.1:8765]` (serveur avec requêtes partielles pour le visualiseur, P15). `eta`, `briefing`, `fetch` attendent leurs tickets.

```sh
SKIMAP_PERSONAL_SOURCES=1 skimap build-all cervieres
skimap zones-at chapelle-des-bois 46.5952 6.1118 2027-01-10T22:00
skimap route cervieres 44.85894,6.725006 44.84465,6.8157 --date 2027-01-15 --start 13:00
# 11.36 km, D+ 462 m, D− 145 m ; temps estimé 2h15 ; retour au plus tard 17:19, arrivée 15:41
```

`build-all <site>` lance dans l'ordre des dépendances : lecture de l'extrait OpenSnowMap (partagée), `pistes`, `domaines`, `terrain`, `snow`, `refuges`, `zones`, `avalanche-exposure` (après `terrain` et `pistes`), `routing` (après `pistes` et `terrain`), `offpiste-prepare` (après `terrain`, `snow`, `zones` et `pistes`), `coverage`, `bera`, `meteo` et `status-poll` (seulement avec `personal_sources` ; `status-poll` après `pistes`, dont les voies donnent la géométrie des pistes de Cervières), `avalanche-historic` (après `pistes`), `assist` (jour courant et 3 suivants, trois demandes de boucles, sorties sur les itinéraires de preuve), puis `build-pack` et `export` (après `build-pack`). `voice` n'en fait pas partie (binaire pockettts.cpp et GPU). Une ligne par étape : temps et chiffres clés ; une étape dont une dépendance a échoué est sautée ; code de sortie non nul si une étape échoue. Avec le cache rempli et `SKIMAP_PERSONAL_SOURCES=1` (15/11/2026, 21 étapes, après les corrections du jour) : 99,3 s à Cervières, dont 81,4 s pour `avalanche-exposure` (le cas qui domine), 3,0 s de lecture de l'extrait, 4,5 s d'export ; 22,0 s à Chapelle-des-Bois (exposition 6,2 s). Packs : 45,1 et 41,7 Mo ; exports : 28,2 et 15,2 Mo (tronçons d'exposition aux sommets OSM au lieu d'un sommet par mètre : −1,2 et −6,4 Mo de pack, −3,8 et −17,5 Mo d'export). Quand le cache du widget Nordic France (3 h) a expiré, `domaines` relit les 9 pages d'associations du recensement à 1 requête par minute : 493 s, et `build-all` passe à 589 s à Cervières ; c'est alors le cas qui domine. `offpiste-prepare` (seul ou dans `build-all`) écrit un `snow_<date>.skr` par couche de neige reçue (par défaut la couche `snow` du jour, sinon `--snow-dir …`) et garde au plus 7 dates : celles de l'exécution, puis les plus récentes listées par l'`offpiste.meta.json` précédent ; toutes sont listées dans `offpiste.meta.json` (`stats.snow`, la plus récente en tête), tout autre `snow_<date>.skr` est supprimé (`stats.snow_retention`).

La version de Rust est figée dans `rust-toolchain.toml` (1.99.1, avec rustfmt, clippy et la cible Android) ; `rustup toolchain install` l'installe. La CI (`.github/workflows/ci.yml`) lance fmt, clippy, les tests (dont clippy et tests de `skimap-core` et `skimap-pack` avec la fonction `pipeline`) et la compilation Android.

## Android (F4, #4)

Prérequis : cible `aarch64-linux-android` (via `rust-toolchain.toml`), `cargo install cargo-ndk --locked` (4.1.2), NDK 28.2 dans `~/Android/Sdk/ndk/28.2.13676358`.

```sh
export ANDROID_NDK_HOME=~/Android/Sdk/ndk/28.2.13676358
cargo ndk -t arm64-v8a --platform 34 build --release -p skimap-cli
file target/aarch64-linux-android/release/skimap
# ELF 64-bit LSB pie executable, ARM aarch64, interpreter /system/bin/linker64, for Android 34
```

Exécution sur le Pixel (téléphone branché, débogage USB autorisé) :

```sh
adb devices
adb push target/aarch64-linux-android/release/skimap /data/local/tmp/skimap
adb shell chmod 755 /data/local/tmp/skimap
adb shell /data/local/tmp/skimap --version
adb shell /data/local/tmp/skimap sites
adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data ./skimap fetch-url https://geo.api.gouv.fr/communes/25121 --source geo-api'
```

Le critère de F4 (sortie du binaire exécuté sur le Pixel, collée dans le ticket) reste à faire, téléphone branché.

## Git

- Chaque agent travaille dans son propre worktree, sur sa branche, et commite localement ; il ne pousse pas et n'ouvre pas de PR. L'orchestrateur vérifie et fusionne dans `main`.
- Les messages de commit citent les tickets (`Refs #14`) ; aucune ligne d'attribution d'IA.
- La signature 1Password n'étant pas toujours disponible : `git -c commit.gpgsign=false commit`.
- Ne jamais versionner `data/`, `research/raw/`, `research/transcripts/`, `target/`.
