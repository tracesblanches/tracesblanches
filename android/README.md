# skimap pour Android (K1 #51, K12 #115, K15 #123, K2 #52, H2 #133, VO6 #129, Q8 #108, E1 #125)

App Kotlin à une seule activité (Compose, Material 3) : la carte d'abord, MapLibre Native, dans les **styles validés par l'utilisateur** (`styles/`, K15) : fond, SRN ou guidage, clair ou sombre. Les packs sont lus par le noyau Rust (`crates/ffi`, UniFFI) : instanciation des styles sur les packs, fiches, catalogue des 219 domaines, plan et installation des mises à jour, en appels groupés. Les packs se téléchargent et se mettent à jour depuis le serveur (K2), puis tout marche hors ligne. Interface en français et en anglais (langue du téléphone, anglais par défaut).

## Sommaire

1. [L'écran carte](docs/carte.md) : styles validés, mode libre sans couture, couches, légende, fiches
2. [Téléchargement et mise à jour](docs/telechargement.md) : module réseau, synchro, WorkManager, carte de France, voix par langue, schémas des données à venir
3. [Vérifier et mesurer](docs/verifier.md) : tests JVM et Compose, captures, test instrumenté avec le serveur local, banc

## Prérequis (poste)

JDK 21, SDK Android (plateforme 37), NDK 28.2.13676358, Rust de `rust-toolchain.toml`, cargo-ndk 4.1.2 :

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
echo "sdk.dir=$HOME/Android/Sdk" > android/local.properties
```

Gradle construit la bibliothèque Rust lui-même (tâches `cargoNdk`, `cargoHost`, `uniffiBindings`), copie les styles validés dans les assets (`styleAssets`), et pour les tests écrit le pack de test en deux versions et le paquet de voix anglais (`testPack`, `testPackV2`, `testVoicePack`), construit `skimap-server` (`cargoServer`) et y publie ces packs (`testStore`).

## Sur le Pixel 8 / 8 Pro

Débogage USB activé, téléphone branché, depuis la racine du dépôt :

```sh
adb devices                                    # le Pixel doit apparaître « device »
cd android && ./gradlew :app:assembleDebug && cd ..
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n fr.skimap.app/.MainActivity
```

Premier lancement : « Aucun pack sur ce téléphone » → « Domaines de France et téléchargements » → un domaine publié → « Télécharger ». Les gros packs attendent le Wi-Fi, sauf « Utiliser les données mobiles ». Pour copier un pack à la main (développement) :

```sh
D=/sdcard/Android/data/fr.skimap.app/files/packs
adb push data/out/cervieres/pack/. $D/cervieres/
adb shell am start -S -W -n fr.skimap.app/.MainActivity -e pack libre       # mode libre : tous les packs
adb shell am start -S -W -n fr.skimap.app/.MainActivity -e pack cervieres   # un domaine
adb logcat -s skimap        # « packs … opened in N ms », « map … fully rendered in N ms », « sync … »
```

## Architecture

- `data/` : `PackRepository` (Rust, `Dispatchers.IO`, un appel par ouverture et par style), `MapStyles` (pratique, thème, guidage → asset), `DomainCatalog`, `Downloads`, `SettingsStore`, `Locator` (une position, au toucher).
- `sync/` : `PackSyncer`, `ServerCatalog`, `PackSync` et `PackSyncWorker` (WorkManager), `WorkManagerDownloads` ; module `:sync` : `HttpSource`, seul code réseau.
- `ui/` : `MapViewModel` et `DomainsViewModel` (`StateFlow`), `MapScreen`, `MapSheet`, `CardSheet`, `PackTexts` (textes traduits), `domains/DomainPicker`, thème à fort contraste ; `ui/map/` : `PackMap` (style rechargé seulement au changement de packs ou de style), `MapEngine` (MapLibre démarré hors du fil principal).
- Textes : `res/values` (anglais, défaut) et `res/values-fr` ; fiches traduites par le noyau (`Lang`).
- Sobriété (E1) : rien en arrière-plan hors WorkManager sous contraintes ; aucune minuterie ; StrictMode sans accès disque au démarrage ; R8 et profil de référence en release.
- Service de localisation, guidage, voix (K4, rapport 09) : agent B.

## Limites

- Téléphone non branché : vérifié sur émulateur Pixel 8 (Android 17, x86_64, pages de 16 Ko).
- Packs par domaine seulement : le serveur ne publie pas encore de pack par zone (cadre, massif). Deux packs qui se recouvrent dessinent deux fois les couches translucides (pentes, exposition) dans le recouvrement.
- Les styles validés dessinent le fond Plan IGN : un pack sans Plan IGN n'affiche que ses couches (les sources absentes sont retirées et listées dans `MapStyle.missing`).
- Paquets de voix à part et zones de redevance : côté app prêts et testés sur données de test ; attendent le serveur (catalogue `voice`), les données (crate `stations`) et une couche de style.
