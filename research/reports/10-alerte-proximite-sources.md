# 10 — Sources du rapport alerte et groupe par proximité (lues le 15/11/2026)

Chaque entrée donne le lien, puis l'extrait ou la valeur utilisée. « Code AOSP » : fichier lu sur android.googlesource.com, `packages/modules/Bluetooth`, branche `main`, lignes indiquées. « Résumé » : vu seulement à travers un résumé de recherche, pas sur la page elle-même.

## Matériel
- **P1** Pixel 8 et 8 Pro : https://support.google.com/pixelphone/answer/7158570 (texte extrait).
  - Les deux : « Bluetooth® v5.3 with dual antennas » ; « Wi-Fi 7 (802.11be) » (UE).
  - 8 Pro seulement : « Ultra-Wideband chip for accurate ranging and spatial orientation ».
  - Pixel Watch 2 : https://support.google.com/googlepixelwatch/?p=specs (alias g.co/pixelwatch/specs) : « Bluetooth® 5.0 », « Wi-Fi 802.11 b/g/n 2.4GHz », « 306 mAh (typical) », aucune ligne UWB.
- **P2** OpenDroneID, liste communautaire de téléphones testés : https://github.com/opendroneid/receiver-android/blob/master/supported-smartphones.md
  - Pixel 8 (Tensor G3, Android 14) : critère d'élimination LR ✅ 1/2024, réception LR ❌ 1/2024, Wi-Fi Beacon ✅, Wi-Fi NAN ✅ ; « Long range support is claimed but the signals are never received ». Même verdict pour les Pixel 6 et 7.
  - Pixel 9 (Android 16) : LR ✅ 6/2026.
  - Le critère d'élimination, c'est « Long Range (PHY Coded) and Extended advertisement are both labeled YES » dans nRF Connect.
  - « Some Android phones claim to support Long Range … but in reality, they only support advertising ».
## API Android
- **P3** https://developer.android.com/reference/android/bluetooth/BluetoothAdapter
  - `isLeCodedPhySupported()`, `isLeExtendedAdvertisingSupported()`, `isOffloadedFilteringSupported()`, `isOffloadedScanBatchingSupported()`.
  - `getLeMaximumAdvertisingDataLength()` : « if LE Extended Advertising feature is supported, 0 otherwise ».
  - `disable()` : applications « not allowed to enable/disable Bluetooth » ; cible TIRAMISU et plus : « will always fail ».
- **P4** https://developer.android.com/reference/android/bluetooth/le/AdvertisingSetParameters
  - INTERVAL_LOW = 160 (« around every 100ms … should not be used for continuous background advertising ») ; MEDIUM = 400 (« 250ms ») ; HIGH = 1600 (« 1000ms », par défaut).
  - TX_POWER : ULTRA_LOW −21, LOW −15, MEDIUM −7, HIGH 1 ; `TX_POWER_MAX_AVAILABLE` (API 37) = 20.
- **P5** https://developer.android.com/reference/android/bluetooth/le/ScanSettings
  - LOW_POWER : « This mode is enforced if the scanning application is not in foreground ».
  - ALL_MATCHES_AUTO_BATCH : lots écran éteint, délai ≥ `AUTO_BATCH_MIN_REPORT_DELAY_MILLIS` = 600000 ; FIRST_MATCH et MATCH_LOST.
  - PHY_LE_ALL_SUPPORTED : « 1Mbit and LE Coded PHYs if supported ».
- **P6** Code AOSP `android/app/src/com/android/bluetooth/le_scan/ScanManager.java` :
  - l. 85-97 : écran éteint LOW_POWER 512/10240 ms, BALANCED 183/730 ; normal LOW_POWER 140/1400, BALANCED 183/730, LOW_LATENCY 100/100 ;
  - l. 120 : `FOREGROUND_IMPORTANCE_CUTOFF = IMPORTANCE_FOREGROUND_SERVICE` ;
  - l. 122-123 : arrière-plan et rétrogradation forcée = SCAN_MODE_LOW_POWER ;
  - l. 405-415 : « Cannot start unfiltered scan in screen-off » ;
  - l. 698-718 : écran éteint, LOW_POWER devient SCREEN_OFF, BALANCED devient SCREEN_OFF_BALANCED, LOW_LATENCY reste ;
  - l. 1334-1336 : FIRST_MATCH exempté du délai ;
  - l. 1550-1572 : délai dépassé, le scan non filtré passe en opportuniste et le scan filtré est rétrogradé.
- **P7** Code AOSP `btservice/AdapterService.java` l. 6975-6979 : `DEFAULT_SCAN_QUOTA_COUNT = 5`, fenêtre `30 * SECOND_IN_MILLIS`, `DEFAULT_SCAN_TIMEOUT_MILLIS = 10 * MINUTE_IN_MILLIS`. `le_scan/ScanController.java` l. 1113-1121 : `SCAN_FAILED_SCANNING_TOO_FREQUENTLY`. `le_scan/AppScanStats.java` l. 830-844.
- **P8** https://source.android.com/docs/core/connect/bluetooth/ble
  - « BLE scanning and filter-matching on the Bluetooth controller » ; « Batch scans reduce how often the AP wakes up ».
  - « For an OnFound event, the main AP wakes up upon the discovery of a specific device ».
- **P9** https://developer.android.com/develop/connectivity/bluetooth/bt-permissions : SCAN, ADVERTISE et CONNECT sont des permissions d'exécution (« Nearby devices ») ; « If you include neverForLocation … some BLE beacons are filtered from the scan results ».
- **P10** https://developer.android.com/develop/connectivity/bluetooth/ble/background : `startScan()` avec PendingIntent ; « Scheduling periodic scans to find devices is discouraged » ; FGS `connectedDevice`.
- **P11** https://developer.android.com/develop/background-work/services/fgs/service-types : `connectedDevice` exige que l'une des permissions BLUETOOTH_CONNECT, ADVERTISE ou SCAN soit accordée (ou CHANGE_WIFI_STATE déclarée…) ; les types de FGS se déclarent dans la Play Console (« Policy > App content »).
- **P12** https://developer.android.com/about/versions/16/behavior-changes-all : « Improved bond loss handling ». https://developer.android.com/about/versions/17/behavior-changes-all : re-appairage autonome. https://developer.android.com/about/versions/17/behavior-changes-17 : ACCESS_LOCAL_NETWORK ; RFCOMM `read()` renvoie −1. Les pages 15 lues ne parlent pas de BLE.
- **P13** https://developer.android.com/develop/connectivity/wifi/wifi-aware
  - « This clustering consumes resources and battery » ; « might not be available because the user has disabled Wi-Fi or Location ».
  - Messages « limited to about 255 bytes » ; « higher throughput rates across longer distances than Bluetooth ».
  - Mode instantané : 30 s ; NEARBY_WIFI_DEVICES.
- **P14** https://developers.google.com/nearby/connections/overview : « fully-offline peer-to-peer » ; « Bluetooth, BLE, and Wi-Fi ». https://developers.google.com/nearby/connections/strategies : Cluster M-to-N « within radio range (~100m) … lower bandwidth ». Le guide de démarrage demande le SDK Play services ; rien sur l'arrière-plan ni la batterie.
- **P15** https://developer.android.com/develop/connectivity/uwb : « accuracy of 10 cm » ; paramètres échangés « using a secure out-of-band (OOB) mechanism … such as BLE » ; appareils « Pixel Pro (6 pro and newer) » ; démarrer une session exige l'avant-plan.
- **P16** Code AOSP `device/google/shusky` (`shiba` et `husky/overlay/frameworks/base/core/res/res/xml/power_profile.xml`) : aucune entrée `bluetooth.*` ; `wifi.controller.idle` 38, `rx` 57, `tx` 550 mA. Rapport 09 (S10) : `cpu.idle` 28,6 mA ; 1 %/h = 45,8 mA.
## Propagation et portée
- **P17** Leith et Farrell, TCD, https://arxiv.org/pdf/2006.06822
  - Rotation d'une personne : « varies by around 20dB » ; à 1 m, ≈ −60 dB face au téléphone, −80 dB de dos ; poche arrière : « around 15dB lower ».
  - Orientation des téléphones : « around 20dB » ; « standard deviation is around 5dB ».
- **P18** https://blog.nordicsemi.com/getconnected/tested-by-nordic-bluetooth-long-range (nRF52840, 0 dBm, dehors) : 1M 654,92 m, Coded 1300 m ; paquets à ≈ −93 et −101 dBm.
- **P19** https://www.bluetooth.com/learn-about-bluetooth/key-attributes/range/
  - « More than a kilometer. Less than a meter. » ; sensibilité minimale « -70 dBm to -82 dBm » ; Coded « -103 dBm » en moyenne.
  - Émission −20 à +20 dBm ; antennes −10 à +10 dBi.
- **P20** Hejselbæk et al., IEEE Access 6 (2018), https://vbn.aau.dk/ws/files/293132166/08468151.pdf : forêt à 917,5 MHz, « foliage excess loss being the dominant loss mechanism » sur environ 1000 m ; modèles MED L = A·f^B·D^C (Weissberger). Coefficients non relus (tableau en image).
- **P21** GAEN, Bluetooth Specification v1.2, https://covid19-static.cdn-apple.com/applications/covid19/current/static/contact-tracing/pdf/ExposureNotification-BluetoothSpecificationv1.2.pdf
  - ADV_NONCONN_IND ; adresse « Random Non-resolvable », rotation « greater than 10 minutes and less than 20 minutes », synchrone avec l'identifiant.
  - Intervalle « 200-270 milliseconds » ; découverte « within 5 minutes », scan « opportunistic » ; « hardware filters shall be used to prevent excessive power drain ».
## Relais existants
- **P22** Bridgefy :
  - Albrecht, Blasco, Jensen, Mareková, CT-RSA 2021, https://ia.cr/2021/214 : « participants of protests should avoid relying on Bridgefy ».
  - Albrecht, Eikenberg, Paterson, USENIX Security 2022, https://www.usenix.org/conference/usenixsecurity22/presentation/albrecht : attaque contre libsignal intégré, « probability of about 50% » ; les utilisateurs restent traçables.
  - Amigo (CCS 2025), via https://corpus.lantern.io/findings/2025-inyangson-amigo__prior-mesh-apps-fail-realistic-conditions/ (résumé).
- **P23** https://briarproject.org/how-it-works/ : « sync via Bluetooth, Wi-Fi or memory cards » ; Briar Mailbox. Relais entre contacts seulement : guide de démarrage, vu en résumé.
- **P24** Berty : https://berty.tech/protocol (Android Nearby, Multipeer, BLE) ; https://berty.tech/ar/blog/bluetooth-low-energy (instabilité BLE Android) ; https://github.com/berty/berty (« should not yet be used to exchange sensitive data »). Tout est vu en résumé.
- **P25** Bitchat : https://blog.trailofbits.com/2025/07/18/building-secure-messaging-is-hard-a-nuanced-take-on-the-bitchat-security-debate/ (résumé) ; empoisonnement du cache corrigé le 28/01/2026 (https://mintaka-2n6.pages.dev/blog/bitchat-cache-poisoning, secondaire).
- **P26** Meshtastic : https://meshtastic.org/docs/overview/range-tests/ (record au sol de 331 km, résumé). FireChat : https://en.wikipedia.org/wiki/FireChat (secondaire).
- **P27** https://support.google.com/android/answer/3265955 : « With network everywhere … busy and remote areas » ; « like the Pixel 8 series … can still locate the phone for several hours after it's turned off ».
## Fréquentation
- **P28** Nordic France, bilan 2025-2026, https://www.nordicfrance.fr/content/uploads/2026/07/Infos-presse-20252026-1.pdf : « 2.5 M » journées skieurs, 13,9 M€, 169 jours, « les 160 domaines » ; « 1 pass saison = 25 JS » ; « ne compte que les accès payant ».
## Secours, droit, vie privée
- **P29** https://www.service-public.gouv.fr/particuliers/vosdroits/F33954 : 112 « utilisable dans tous les pays de l'Union européenne », depuis un fixe ou un portable, gratuit. 114 : personnes sourdes, malentendantes ou aphasiques, SMS gratuit.
- **P30** https://developer.android.com/reference/android/content/Intent : ACTION_CALL : « this Intent cannot be used to call emergency numbers. Applications can dial emergency numbers using ACTION_DIAL ».
- **P31** Rapport 06 (§4) : AML activé en France en 2020 (EENA 2023) ; pas de SOS par satellite sur Pixel 8 (https://support.google.com/pixelphone/answer/15254448).
- **P32** https://www.ffrandonnee.fr/s-informer/actualites/ces-appareils-connectes-qui-generent-de-faux-appels-au-secours-en-montagne (17/06/2026) : « Les fausses alertes se multiplient », constat des « gendarmes de Haute-Montagne » ; aucun chiffre.
- **P38** https://www.cnil.fr/fr/reglement-europeen-protection-donnees/chapitre1 : art. 4(1), « des données de localisation ».
## DVA
- **P33** Genswein, Atkins (« Vice President of the ICAR Avalanche Commission ») et al., ISSW 2013, https://arc.lib.montana.edu/snow-science/objects/ISSW13_paper_P1-34.pdf
  - « 20cm safety distance in transmit and 50cm in receive mode ».
  - « actively searching rescuers must completely turn off cellular phones … even in "air plane mode" in distances up to 100cm interferences ».
- **P34** Meister et Dammert, ISSW 2014, https://arc.lib.montana.edu/snow-science/objects/ISSW14_paper_P4.13.pdf : téléphone à 40 cm = signal d'un enseveli à 50 m ; « only at 80cm distance, the interference falls below the safety threshold » ; « turn off mobile phones ».
- **P35** Edgerly, ISSW 2023, https://arc.lib.montana.edu/snow-science/objects/ISSW2023_P2.60.pdf : « 20/50 Rule … adequate » ; « Smartphones created no EMI effects outside 10cm … no measurable difference … airplane mode ».
- **P36** ICAR-IKAR-CISA (14/10/2006), https://alpine-rescue.org/articles/109--avalanche-safety-devices-and-systems : « The efficiency of the transceiver in combination with probe and shovel … has been proven » ; « other systems is based upon personal opinion ». Pas de texte ICAR récent sur les applis trouvé.
- **P37** Avis du Canadian Avalanche Centre (2013) contre les applis DVA, presse seulement : https://globalnews.ca/news/923611/backcountry-users-warned-about-avalanche-transceiver-apps/
