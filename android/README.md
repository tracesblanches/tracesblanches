# skimap pour Android (K1 #51, K12 #115, Q8 #108, E1 #125)

App Kotlin à une seule activité (Compose, Material 3) : la carte d'abord, MapLibre Native lisant les PMTiles des packs (Plan IGN v2 ou OSM), pistes (couleur, forme, nom, sens), état, refuges et points d'eau. Les packs sont lus par le noyau Rust (`crates/ffi`, UniFFI) : style MapLibre complet, symboles, légende, couches, fiches et catalogue des 219 domaines, en appels groupés. Aucune requête réseau (permission INTERNET retirée).

## Prérequis (poste)

JDK 21, SDK Android (plateforme 37), NDK 28.2.13676358, Rust de `rust-toolchain.toml`, cargo-ndk 4.1.2 :

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
echo "sdk.dir=$HOME/Android/Sdk" > android/local.properties
```

Gradle construit la bibliothèque Rust lui-même (tâches `cargoNdk`, `cargoHost`, `uniffiBindings`, `testPack`).

## Sur le Pixel 8 / 8 Pro (demain)

Débogage USB activé, téléphone branché, depuis la racine du dépôt :

```sh
adb devices                                    # le Pixel doit apparaître « device »
cd android && ./gradlew :app:assembleDebug && cd ..
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
# 1. Premier lancement, attendre l'écran « Aucun pack sur ce téléphone » : l'app a créé son
#    dossier (créé par adb, il appartiendrait à adb et resterait illisible pour l'app)
adb shell am start -n fr.skimap.app/.MainActivity
# 2. Copie des packs (45 et 42 Mo), puis relance
D=/sdcard/Android/data/fr.skimap.app/files/packs
adb push data/out/cervieres/pack/. $D/cervieres/
adb push data/out/chapelle-des-bois/pack/. $D/chapelle-des-bois/
adb shell am start -S -W -n fr.skimap.app/.MainActivity -e pack libre       # mode libre : tous les packs
adb shell am start -S -W -n fr.skimap.app/.MainActivity -e pack cervieres   # un domaine
adb logcat -s skimap        # « pack … opened in N ms », « map … fully rendered in N ms »
adb exec-out screencap -p > pixel.png
```

L'app rouvre le dernier mode (libre par défaut). Mettre à jour un pack : recopier son dossier.

## L'écran carte

- Mode libre par défaut (tous les packs installés dans une seule carte) ; mode domaine depuis le menu ou la carte de France (219 domaines du recensement : recherche, « les plus proches » par une position grossière demandée au toucher, état du pack : installé, disponible, pas encore construit).
- Gestes : pincement, double toucher, double toucher puis glisser. Aucun bouton de zoom ; « Vue auto » (48 dp) n'apparaît qu'après un geste. Menu « Carte et légende » et ⓘ (licences) en coins, petits, semi-transparents.
- Menu : mode, fond, couches (refuges, points d'eau, zones sans réseau, noms du fond ; mémorisé par fond, visibilité basculée sans recharger le style ; pistes, sens et état toujours affichés), légende (forme et mot pour chaque couleur), licences.
- Toucher un refuge : fiche (feuille du bas) avec gardiennage, places, équipements, accès, dernier commentaire daté, téléphones à composer (le composeur s'ouvre, l'app n'appelle jamais), liens, source.

Schémas attendus des données à venir (crates `stations`, `refuges`) : couche de pack `stations` (points) : `name`, `emergency` `[{label, phone}]` (112 toujours en premier), `fee_outlets` `[{name, address, phone}]`, `prices` `[{label, price_eur, date}]`, `phones`, `websites` `[{label, url}]`, `source`, `updated` ; fiche prête (`crates/ffi/src/card.rs`). Zones de redevance : couche `fee-zones` (polygones `{fee_id, name, issuer}`) et titre de l'utilisateur mémorisé : interface à faire quand la couche existera.

## Vérifier

```sh
cd android
./gradlew build verifyRoborazziDebug detekt ktlintCheck   # tests JVM et Compose, captures, lint sans avertissement
./gradlew recordRoborazziDebug                            # réenregistre les références de captures
adb push data/out/cervieres/pack/. /data/local/tmp/skimap-packs/cervieres/   # vrais packs (facultatif)
./gradlew :app:connectedDebugAndroidTest                  # émulateur ou téléphone, orchestrateur de tests
cargo run -p skimap-ffi --example catalog                 # régénère assets/catalog.json (recensement)
```

## Mesurer

```sh
./gradlew :benchmark:connectedBenchmarkReleaseAndroidTest   # démarrage à froid jusqu'à la carte affichée
./gradlew :app:generateBaselineProfile                       # app/src/release/generated/baselineProfiles/
```

Sur émulateur, chiffres indicatifs ; l'énergie se mesurera sur le Pixel (Perfetto, rails ODPM).

## Architecture

- `data/` : `PackRepository` (Rust, `Dispatchers.IO`, un appel par ouverture et par fond), `DomainCatalog`, `SettingsStore`, `Locator` (une position, au toucher).
- `ui/` : `MapViewModel` et `DomainsViewModel` (`StateFlow`), `MapScreen` (carte en paramètre), `MapSheet`, `CardSheet`, `domains/DomainPicker`, thème à fort contraste.
- `ui/map/` : `PackMap` (style rechargé seulement au changement de pack ou de fond), `MapEngine` (MapLibre démarré hors du fil principal), `SymbolBitmaps`.
- Sobriété (E1) : rien en arrière-plan, aucune minuterie ; StrictMode sans accès disque au démarrage ; R8 et profil de référence en release.
- Futur service de localisation (K4, rapport 09) : `fr.skimap.app.location`, un seul service de premier plan `location` lancé par un geste, positions par lots au noyau. Pas écrit.

## Limites

- Téléphone non branché le 15/11 : vérifié sur émulateur Pixel 8 (Android 17, x86_64, pages de 16 Ko).
- Pas de suivi de position ni de zoom automatique (K4), pas de téléchargement (K2), pas encore les styles K15.
