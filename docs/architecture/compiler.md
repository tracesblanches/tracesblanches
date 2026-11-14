# Compiler, tester, Android, git

[← Architecture](../architecture.md)

## Compiler et tester

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p skimap-common          # sans la fonction download (côté téléphone)
cargo run -p skimap-cli -- sites
cargo run -p skimap-cli -- fetch-url 'https://geo.api.gouv.fr/communes/05027' --source geo-api
cargo run --release -p skimap-cli -- build-all cervieres
```

## Ligne de commande

Une sous-commande par constructeur de couche, qui appelle la fonction publique de la crate : `pistes`, `domaines` (France puis les sites), `terrain`, `snow [--date]`, `snow-validate [--seasons A B]`, `refuges`, `zones`, `coverage [--release YYYY_TQ] [--archives DIR]`, `bera [--date]`, `meteo`, `status-poll`, `avalanche-historic [--dump FICHIER]`, et `zones-at <site> <lat> <lon> <date>` (C8 sur la couche `zones`). `build-pack`, `route`, `eta`, `briefing`, `export`, `fetch` attendent leurs tickets.

```sh
SKIMAP_PERSONAL_SOURCES=1 skimap build-all cervieres
skimap zones-at chapelle-des-bois 46.5952 6.1118 2027-01-10T22:00
```

`build-all <site>` lance dans l'ordre des dépendances : lecture de l'extrait OpenSnowMap (partagée), `pistes`, `domaines`, `terrain`, `snow`, `refuges`, `zones`, `coverage`, `bera`, `meteo` et `status-poll` (seulement avec `personal_sources`), `avalanche-historic` (après `pistes`). Une ligne par étape : temps et chiffres clés ; une étape dont une dépendance a échoué est sautée ; code de sortie non nul si une étape échoue. Avec le cache rempli : 9,5 s (Cervières), 11,2 s (Chapelle-des-Bois).

La version de Rust est figée dans `rust-toolchain.toml` (1.99.1, avec rustfmt, clippy et la cible Android) ; `rustup toolchain install` l'installe. La CI (`.github/workflows/ci.yml`) lance fmt, clippy, les tests et la compilation Android.

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
