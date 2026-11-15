# 09 — Sources du rapport batterie (lues le 15/11/2026)

Chaque entrée donne le lien, puis l'extrait ou la valeur utilisée. « Code AOSP » : fichier lu sur android.googlesource.com, branche `main`. « Non lu » : vu seulement dans un résumé de recherche, pas sur la page elle-même.
## S1 — Version d'Android sur les Pixel 8
- https://9to5google.com/2026/06/16/google-android-17-pixel-launch/ : Android 17 stable le 16/06/2026 ; la liste comprend « Pixel 8, Pixel 8 Pro ».
- https://9to5google.com/2026/11/10/android-17-qpr2-november-pixel-update/ : QPR2 déployé le 10/11/2026 (résumé de recherche : build Pixel 8 CP4A.261105.040.A1, non lu en entier).
- https://developer.android.com/about/versions/17/behavior-changes-all : « Android 17 (API level 37) ».
## S2 — Fused Location Provider (documentation Google)
- https://developer.android.com/develop/sensors-and-location/location/battery : HIGH_ACCURACY « enables GPS, Wi-Fi, and cell … may cause significant battery drain » ; BALANCED « Very rarely uses GPS » ; LOW_POWER « coarse (city-level) accuracy » ; maxUpdateDelay « several times larger than the interval … delivered in batches ».
- https://developer.android.com/develop/sensors-and-location/location/battery/scenarios : navigation ou sport écran éteint → « Use a foreground service » ; PASSIVE « incurs almost no battery drain ».
- https://developer.android.com/develop/sensors-and-location/location/battery/optimize : timeouts (`setDurationMillis`), lots, mises à jour passives.
- https://developers.google.com/android/reference/com/google/android/gms/location/LocationRequest.Builder : délai max « controls location batching … may (but is not required to) save power » ; distance min « may also allow additional power savings » ; `setWaitForAccurateLocation` vrai par défaut ; IMPLICIT_MIN_UPDATE_INTERVAL = « half the interval ».
- https://developers.google.com/android/reference/com/google/android/gms/location/Priority : PASSIVE « no extra power will be used ».
## S3 — GNSS dans le framework et le HAL
- https://developer.android.com/reference/android/location/LocationRequest : lots « if the maximum update delay is equal to or greater than twice the interval » ; « Not all devices … support batching » ; « substantial power savings ».
- Code AOSP `services/core/java/com/android/server/location/gnss/GnssLocationProvider.java` : lot matériel si `getBatchSize() > 1` et `batchLengthMs / 2 >= batchIntervalMs` ; `GPS_POLLING_THRESHOLD_INTERVAL = 10 * 1000` (« if above then we cycle the GPS driver. Typical hot TTTF is ~5 seconds ») ; `hibernate()` éteint le GNSS jusqu'au prochain fix.
- Code AOSP `services/core/java/com/android/server/location/provider/LocationProviderManager.java` : distance minimale = « dropped delivery - too close » (filtre de livraison).
- https://developer.android.com/reference/android/location/GnssMeasurementRequest.Builder : full tracking « switches off duty cycling … will consume more power » ; « Full tracking requests always override non-full tracking requests ».
- Code AOSP `hardware/interfaces/gnss/aidl/…/IGnss.aidl` : `lowPowerMode` (« must not … run power hungry operations … for more than one second per interval ») ; `location/java/android/location/LocationRequest.java` : `setLowPower` est `@SystemApi` et exige `LOCATION_HARDWARE`.
- Code AOSP `IGnssBatching.aidl` : « primarily used when the AP is asleep and the device can batch locations in the hardware » ; `GnssPowerStats.aidl` : énergies séparées « single frequency band » et « multiple frequency bands ».
## S4 — Services de premier plan
- https://developer.android.com/develop/background-work/services/fgs/service-types : type `location` et FOREGROUND_SERVICE_LOCATION ; « you cannot create a location foreground service while your app is in the background, unless you've been granted ACCESS_BACKGROUND_LOCATION ».
- https://developer.android.com/develop/background-work/services/fgs/timeout : « Currently, this restriction only applies to dataSync and mediaProcessing » (6 h par 24 h).
- https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start : exemption « receives an event that's related to geofencing or activity recognition transition » ; FGS « while-in-use » : démarrées app visible, ou via une notification ou un widget.
- https://developer.android.com/about/versions/16/behavior-changes-all : « jobs that are executing concurrently with a foreground service will adhere to the job runtime quota ».
## S5 — Android 17 : audio en arrière-plan
- https://developer.android.com/about/versions/17/changes/bg-audio : « must have a visible activity or must be running a foreground service » ; cible 37 : « foreground service that has while-in-use (WIU) capabilities » ; sinon « playback … fail silently » et le focus renvoie `AUDIOFOCUS_REQUEST_FAILED`.
## S6 — Doze, veille, wakelocks
- https://developer.android.com/training/monitoring-device-state/doze-standby : restrictions de Doze ; App Standby ne s'applique pas si l'app a « a foreground service » ; exemption directe réservée aux cas où Doze casse la fonction principale.
- https://developer.android.com/about/versions/nougat/android-7.0-changes : Doze en mouvement (« traveling in a user's pocket ») : réseau, jobs, synchros ; à l'arrêt : « WakeLock, AlarmManager alarms, GPS, and Wi-Fi scans ».
- Code AOSP `PowerManagerService.java` (`setWakeLockDisabledStateLocked`) : wakelock coupée seulement si `procState > PROCESS_STATE_BOUND_FOREGROUND_SERVICE`. `core/java/android/net/NetworkPolicyManager.java` : réseau permis si `procState <= FOREGROUND_THRESHOLD_STATE` (= BOUND_FOREGROUND_SERVICE).
- Code AOSP `StationaryThrottlingLocationProvider.java` : « Throttles location providers completely while the device is in doze and stationary », sauf `QUALITY_HIGH_ACCURACY`.
- https://developer.android.com/topic/performance/vitals/wakelock : « 2 or more hours in a 24-hour period » ; exemption des wakelocks créées par « audio, location, and JobScheduler user-initiated APIs » ; « more than 5% of app sessions … can affect your app's visibility ». https://android-developers.googleblog.com/2026/03/battery-technical-quality-enforcement.html (résumé de recherche : sanctions depuis le 1er mars 2026 ; non lu en entier).
- https://developer.android.com/topic/performance/power/power-details : FGS = réseau « No restrictions » ; tableau des buckets.
## S7 — Géorepérage
- https://developer.android.com/develop/sensors-and-location/location/geofencing : « minimum radius … 100 - 150 meters » ; « uses only network location » ; « Usually the latency is less than 2 minutes … (up to 6 minutes) » ; ACCESS_BACKGROUND_LOCATION ; 100 géorepères par app.
## S8 — Capteurs
- https://developer.android.com/develop/sensors-and-location/location/transitions : IN_VEHICLE, ON_BICYCLE, RUNNING, STILL, WALKING ; « latency of event detection might vary by device ».
- https://source.android.com/docs/core/interaction/sensors/sensor-types : mouvement significatif « Implement only the wake-up version » et « must be low power » ; proximité « Usually defined as a wake-up sensor ».
- https://source.android.com/docs/core/interaction/sensors/batching : « the AP can enter the suspend mode even while sensor data is being collected » ; une FIFO sans réveil pleine « must wrap around ».
- CDD Android 16 §7.3 (https://source.android.com/docs/compatibility/16/android-16-cdd) : significant motion, détecteur et compteur de pas « less than 4 mW » ; pression `hifi_sensors` : « batching power consumption not worse than 2 mW », FIFO ≥ 300.
- https://developer.android.com/reference/android/hardware/SensorManager : `maxReportLatencyUs` « reducing power consumption » ; `requestTriggerSensor` à un coup ; « the system will not disable sensors automatically when the screen turns off ».
## S9 — Aide Pixel (support.google.com/pixelphone)
- https://support.google.com/pixelphone/answer/6187458 : Économiseur « Location services stop when your screen is off » ; « Smooth display turns off » ; « Power-saving audio codec for compatible Bluetooth headsets » ; l'Économiseur extrême met la plupart des apps en pause (sauf les « Essential apps »).
- https://support.google.com/pixelphone/answer/7015477 et https://support.google.com/pixelphone/answer/6090612 : garder Adaptive Battery ; thème sombre ; couper l'AOD et Smooth Display. https://support.google.com/pixelphone/answer/6187455 : « Use GPS for long periods », mode avion.
## S10 — Matériel et profil d'énergie du Pixel 8
- https://support.google.com/pixelphone/answer/7158570 : Pixel 8 : 4575 mAh (typique, minimum 4485), « Smooth Display (60–120 Hz) » ; Pixel 8 Pro : 5050 mAh (minimum 4950), « 1–120 Hz » ; « Dual Band GNSS » ; baromètre et proximité.
- Code AOSP `device/google/shusky` (`shiba` et `husky/overlay/…/power_profile.xml`, dernier commit du dépôt en mars 2025) : `cpu.suspend` 10, `cpu.idle` 28.6, `cpu.active` 12.37, `gps.signalqualitybased` 28 / 5, `screen.on` 98, `screen.full` 470, `ambient.on` 32, `audio` 75, `modem.controller.tx` 153 à 471 mA ; `gps.voltage` 3700 mV.
- Code AOSP `core/java/com/android/internal/os/PowerProfile.java` : `cpu.idle` = « Power consumption when CPU is awake (when a wake lock is held) … in addition to POWER_CPU_SUSPEND ». https://source.android.com/docs/core/power/values : ce sont des valeurs de modèle (batterystats), pas des mesures publiées.
## S11 — Économiseur de batterie dans AOSP
- Code AOSP `core/res/res/values/config_battery_saver.xml` : `config_batterySaver_full_locationMode` = 3 (LOCATION_MODE_FOREGROUND_ONLY). `SystemAppForegroundHelper.java` : premier plan = importance ≥ `IMPORTANCE_FOREGROUND_SERVICE`. La surcouche Pixel n'a pas été lue : `adb shell dumpsys power` reste à faire.
## S12 — CDD Android 16 §7.3.3 GPS
- Fix en ≤ 10 s « when connected to a 0.5 Mbps or faster data speed internet connection » ; puis « within 5 seconds … up to an hour after … even when the subsequent request is made without a data connection ».
## S13 — Coût du GNSS mono et bi-fréquence
- https://arxiv.org/pdf/1910.13041 (Karki et Won, Univ. of Memphis) : Mi 8 (BCM47755) 318 ± 32 mJ, Redmi Note 7 232 ± 20 mJ par seconde de mise à jour ; « 37% more power … outdoors, and 28% … indoors » ; mesure Monsoon.
- https://eenewseurope.com/news/7nm-gnss-receiver-wearables-cuts-power-5x : BCM4778 « 4mW for the L1 band and 6mW for both L1 and L5 » (22/09/2021).
- https://www.garmin.com/en-US/blog/outdoor/garmin-engineer-talks-satiq-longer-battery-life-same-precise-tracking/ : fēnix 7X : 63 h toutes constellations, « drops to 36 hours » en multi-bande, GPS seul 89 h ; « multi-band is used only about 15% of the time ».
## S14 — Apps de navigation et de sport
- https://docs.osmand.net/docs/user/troubleshooting/general : sur un « mid-level device », veille 0,5 %/h, écran 6 %/h, GPS 5 %/h, enregistrement écran éteint 6 %/h, navigation écran allumé 12 %/h.
- https://www.locusmap.app/?p=9826 (2016) : GPS auto-off ; d'après un commentaire de 2023, l'option n'existe plus sous Android 12 et suivants.
- https://www.komoot.com/adventure-hub/4MasE6DPzRyY4tHUgDqF6w/4-ways-to-use-komoot-during-your-adventure : « navigation while in flight mode saves your phone's battery ».
- https://support.strava.com/en-us/articles/15402055-extending-battery-life-on-android : GPS en mode avion (sauf certains Motorola) ; « more than 5 hours ».
- https://slopes.helpscoutdocs.com/article/81-all-day-battery-life-tips-android : « don't use airplane mode, as it will degrade GPS accuracy » ; « a cold phone gets worse battery life ».
- https://developers.google.com/maps/documentation/navigation/android-sdk/optimize-power : « Screen rendering usually causes the highest power consumption » ; « cell radios remain active for 20-30 seconds » ; guidage sans carte ; thème sombre.
## S15 — Audio
- https://developer.android.com/media/media3/exoplayer/battery-consumption : « For short audio playbacks … audio does not have a significant impact on power ».
- https://developer.android.com/media/optimize/audio-focus : TRANSIENT_MAY_DUCK pour les indications de navigation ; « voice guidance … are not ducked » ; cible 35 : focus refusé sans FGS ni app au premier plan.
- LE Audio et LC3, environ deux fois moins de consommation : Bluetooth SIG et Nordic (résumé de recherche, **non vérifié**).
## S16 — Écran et notifications
- https://developer.android.com/develop/ui/views/notifications/live-update : conditions (POST_PROMOTED_NOTIFICATIONS, `setRequestPromotedOngoing`, notification permanente, `ProgressStyle`) ; « Active navigation » ; « bridged to some connected Wear OS devices ».
- https://developer.android.com/media/optimize/performance/frame-rate : `setFrameRate(60…)` « to reduce power usage » ; l'Économiseur peut limiter le taux de rafraîchissement.
- https://www.purdue.edu/newsroom/releases/2021/Q3/dark-mode-may-not-save-your-phones-battery-life-as-much-as-you-think,-but-there-are-a-few-silver-linings.html : 3 à 9 % entre 30 et 50 % de luminosité ; 39 à 47 % à 100 % ; Pixel 2, 4 et 5, Moto Z3 ; MobiSys 2021.
## S17 — Réseau
- https://developer.android.com/develop/background-work/background-tasks/persistent/getting-started/define-work : contraintes NetworkType, BatteryNotLow, RequiresCharging.
- Code AOSP `core/java/android/provider/Settings.java` (Global) : « Applications can read these but are not allowed to write » (`AIRPLANE_MODE_ON`).
- Code AOSP `frameworks/opt/telephony/…/emergency/RadioOnHelper.java` : « Turning off airplane mode for emergency call ».
- Coût d'une recherche de réseau en zone blanche : aucune mesure publiée trouvée (Ding et al., SIGMETRICS 2013 ne couvre que la 3G et le Wi-Fi).
## S18 — Froid
- https://support.google.com/pixelphone/answer/15679416 : « If your phone's temperature is below 0°C / 32°F, it may shut down suddenly ».
- https://pmc.ncbi.nlm.nih.gov/articles/PMC10336268 (Chen et al., The Innovation, 2023) : LFP/graphite « could only provide available 70% and 60% capacities » à −10 et −20 °C ; résistance interne en hausse.
- https://developer.android.com/reference/android/os/BatteryManager : EXTRA_TEMPERATURE, BATTERY_HEALTH_COLD, CHARGE_COUNTER (µAh), CURRENT_NOW (µA, négatif à la décharge). Code AOSP `hardware/interfaces/health/aidl/…/HealthInfo.aidl` : `batteryTemperatureTenthsCelsius`.
## S19 — Montre
- https://developer.android.com/health-and-fitness/guides/health-services/active-data : écran éteint, les données sont « less often, but still sampled at the same frequency » ; « every 150 seconds » ; BatchingMode « impact to battery ».
- https://developer.android.com/training/wearables/apps/power : GPS « High », Bluetooth « Medium », réseau « Very high » ; Data Layer : « each transmission uses some power ».
- https://developer.android.com/training/wearables/apps/location-detection : le FLP de la montre « may use location data from the phone » ; Health Services est conseillé pour le sport.
- https://support.google.com/googlepixelwatch/answer/13579780 : « Google Pixel Watch has 8-10 hours of battery life with continuous GPS use ».
## S20 — Outils de mesure
- https://developer.android.com/studio/profile/power-profiler : rails ODPM (CPU Big, Mid et Little, Display, GPS, Cellular, WLAN, Memory, Sensor core…) « on Pixel 6 and subsequent Pixel devices » ; ces rails mesurent tout l'appareil, pas une app.
- https://perfetto.dev/docs/data-sources/battery-counters : `android.power` avec `collect_power_rails: true`, `battery_counters` ; avec le câble USB branché, le courant mesuré devient positif (charge). https://perfetto.dev/docs/concepts/config : `write_into_file`, `perfetto --background`.
- https://developer.android.com/topic/performance/power/setup-battery-historian : « Battery Historian is no longer actively maintained ».
## S21 — Projet skimap
- Ticket C2 #34 (commentaire « Preuve ») : « idem, 1 fix / 5 s | 91,7 / 92,9 % » contre 92,0 / 92,5 % à 1 Hz (σ 15 m + forêt). `crates/core/src/matching.rs` : `ROUTE_HYSTERESIS` 40 m pendant 15 s, retour sous 15 m pendant 5 s ; couloir de 50 m pendant 20 s. `crates/core/src/eta.rs` : `max_speed_ms` 7 m/s. `research/etat-des-lieux.md` lignes 337 à 344 (cas à mesurer).
