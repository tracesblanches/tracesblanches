# Module `sport` : partager la sortie terminée (#63)

Sur le bilan d’une sortie terminée (écran du plan à la fin, et « Bilans des sorties », #97), « Partager la sortie (FIT) » et « Partager en GPX » : le fichier part par le menu de partage d’Android (Strava, Komoot, Garmin Connect, un courriel…), et la sortie est écrite dans Health Connect quand le téléphone l’a et que l’utilisateur l’accorde. Aucun compte, aucune clé, aucun réseau utilisé par l’application. Rien sur la carte, aucun réglage.

| Fonction | Comment |
|---|---|
| Fichiers | Écrits par le cœur Rust (`crates/ffi/src/export.rs`, `export_outing`) dans `cache/sport-share/` : GPX 1.1 (trace, altitude, temps, fréquence cardiaque dans l’extension `TrackPointExtension` v1 de Garmin) et FIT activité (file id, événements du chrono, points, tour, séance, activité ; sport `cross_country_skiing`, sous-sport `backcountry` en ski de randonnée nordique ou pour une sortie hors piste guidée). Distance = celle du bilan (sauts GPS et arrêts écartés), altitude = celle du bilan (baromètre recalé sur le relief ; baromètre seul : dénivelé gardé, pas d’altitude par point). |
| Partage | `ACTION_SEND` du fichier par le `FileProvider` (`${applicationId}.sport.files`, lecture accordée à l’application choisie), dans le sélecteur du système. FIT : `application/vnd.ant.fit` ; GPX : `application/gpx+xml`. |
| Health Connect | Demandé au premier partage seulement (jamais au démarrage) ; refusé, il n’est plus redemandé ; accordé plus tard dans Health Connect, la sortie y est écrite au partage suivant. `ExerciseSessionRecord` type `SKIING` (Health Connect n’a qu’un type de ski) avec son `ExerciseRoute`, `DistanceRecord`, `ElevationGainedRecord`, chacun selon l’autorisation donnée ; identifiants client stables : repartager la même sortie la remplace. |
| Fréquence cardiaque | Lue dans Health Connect (celle d’une montre) sur les heures de la sortie seulement, si l’utilisateur l’accorde, pour la mettre dans le fichier partagé. Rien d’autre n’est lu. |
| Justification | `HealthRationaleActivity` (sous-classée par l’app pour son thème) : ouverte par Health Connect (`ACTION_SHOW_PERMISSIONS_RATIONALE`, alias `VIEW_PERMISSION_USAGE` sur Android 14+). |

## Vérifier

```sh
cargo test -p skimap-ffi --lib export      # FIT relu par fitparser (CRC, points, temps, distance, sport)
cd android
./gradlew :sport:testDebugUnitTest :app:testDebugUnitTest --tests 'fr.skimap.app.ShareOutingTest'
# Émulateur Android 14+ (Health Connect intégré) :
./gradlew :sport:connectedDebugAndroidTest
adb logcat -s skimap   # « #63 health connect: … », « share …: FIT, health connect 3 records, menu true »
```

Tests JVM : `HealthRecordsTest` (séance, itinéraire, distance, dénivelé selon les autorisations), `ShareFlowTest` (faux Health Connect : demande au premier partage seulement, écriture, fréquence cardiaque relue dans un second export, `ACTION_SEND` avec l’URI du `FileProvider` et la lecture accordée ; capture `src/test/screenshots/sport_share.png`). Côté app, `ShareOutingTest` passe par le vrai cœur : GPX validé contre le schéma GPX 1.1 et celui de `TrackPointExtension` v1 (`app/src/test/resources/gpx/`), mêmes points et temps, en-tête FIT.

## Limites

- Ce que chaque application fait du fichier reçu (type d’activité retenu par Strava ou Komoot) n’est vérifiable que sur le téléphone.
- L’application n’enregistre pas elle-même la fréquence cardiaque : elle n’apparaît que si une montre l’a écrite dans Health Connect.
