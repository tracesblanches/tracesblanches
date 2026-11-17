# Vérifier et mesurer (Q8 #108, E1 #125)

[← skimap pour Android](../README.md)

## Tests JVM, Compose, captures, analyse

```sh
cd android
./gradlew build verifyRoborazziDebug detekt ktlintCheck   # tout, lint sans avertissement
./gradlew recordRoborazziDebug                            # réenregistre les références de captures
cargo run -p skimap-ffi --example catalog                 # régénère assets/catalog.json (recensement)
```

- `RustPackRepositoryTest` : le vrai noyau (bibliothèque hôte, JNA), le pack de test et les six styles de `styles/`.
- `PackSyncTest` : le vrai `skimap-server` (tâche `cargoServer`) sert un dépôt où le pack de test a été publié en version 1 puis 2 et le paquet de voix anglais (tâche `testStore`) : premier téléchargement, synchro sans changement (304), delta, reprise après coupure, fichier partiel corrompu, contenus perdus (repli sur la base), serveur injoignable, catalogue gardé hors ligne, paquet de voix à part, plan trop gros sur réseau mesuré.
- `HttpSourceTest` (module `:sync`) : 304, reprise, réponse 200 à une reprise, coupure en cours de corps.
- `MapViewModelTest`, `DomainsTest`, `MapScreenTest` (français), `MapScreenEnglishTest`, `ThemeAndTextsTest`, `CardTest` ; captures Roborazzi en français (clair, sombre, police ×2) et en anglais (`src/test/screenshots/`).
- Côté Rust : `cargo test -p skimap-ffi` (styles instanciés sur le pack de test, mode libre, synchro depuis des paquets faits comme le serveur, catalogue du serveur, fiches FR/EN) et `cargo test --release -p skimap-ffi --test api -- --ignored --nocapture` (les six styles sur les packs réels, seuls et ensemble).

## Test instrumenté (émulateur ou téléphone)

Émulateur Pixel 8 SwiftShader (`-gpu swiftshader_indirect`), lancé détaché, attente au premier plan bornée (`adb wait-for-device`, puis `getprop sys.boot_completed`). Serveur local pour les téléchargements :

```sh
./gradlew :app:testStore :app:cargoServer
SKIMAP_STORE=app/build/generated/testStore SKIMAP_SERVER_DB=/tmp/skimap-test.sqlite SKIMAP_CONTACT_EMAIL=test@example.org \
  setsid app/build/rust/server/skimap-server serve --addr 127.0.0.1:8787 > /tmp/skimap-server.log 2>&1 &
adb push ../data/out/<site>/pack/. /data/local/tmp/skimap-packs/<site>/   # vrais packs (facultatif)
./gradlew :app:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.skimapServer=http://10.0.2.2:8787
kill %1
```

- `PackDisplayTest` : pack de test dans les styles validés (couches dessinées, groupes basculés sans recharger, fiche), vrais packs (fond, SRN, sombre, zoom), mode libre des deux packs.
- `PackDownloadTest` : téléchargement depuis le serveur local par WorkManager, mise à jour différentielle (version 1 copiée, delta), mode avion (travail en attente, carte affichée, puis téléchargement au retour du réseau). Sans l'argument `skimapServer`, ces tests sont sautés et le disent.

## Mesurer

```sh
./gradlew :benchmark:connectedBenchmarkReleaseAndroidTest   # démarrage à froid jusqu'à la carte affichée
./gradlew :app:generateBaselineProfile                       # app/src/release/generated/baselineProfiles/
```

Sur émulateur, chiffres indicatifs ; l'énergie se mesurera sur le Pixel (Perfetto, rails ODPM). Taille et temps d'une synchro : lignes `sync …` de `adb logcat -s skimap` (requêtes, octets, millisecondes).
