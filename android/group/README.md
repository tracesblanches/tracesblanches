# Module `group` : le mode groupe sans réseau (N2, #128)

Stratégie du rapport `research/reports/10-alerte-proximite.md` : publicité BLE seule, trame chiffrée, étiquette de groupe tournante, relais des dernières positions, écoute filtrée par lots **dans le service de localisation de la sortie** (aucun service, fil, wakelock ni minuterie en plus), alertes de séparation.

Le noyau Rust (`crates/group`, exposé par `crates/ffi/src/group.rs`) décide de tout : groupe et QR, trames, table des membres, anti-rejeu, alertes. Ce module applique : radio, écran, lien avec la sortie. Il ne connaît pas les liaisons UniFFI : l'app adapte le noyau à `GroupBackend` (`app/.../group/RustGroupBackend.kt`).

## Ce qui est fait

| Fonction | Comment |
|---|---|
| Groupe | Créé au chalet : clé de 32 octets (`SecureRandom`), prénoms facultatifs (vide : « Membre n » ; le créateur d’abord, 16 au plus), empreinte du plan de journée validé ce jour sur ce téléphone (SHA-256 de `plan.json`, 8 octets). Gardé dans des préférences privées. |
| QR | Un code par membre : `SKIMAP:G1:` + base32 (mode alphanumérique) + `/n` (numéro du membre), matrice calculée par le noyau, dessinée noir sur blanc. Rejoindre = scanner son code (CameraX + ZXing, aucun service Google Play), rien d’autre. |
| Trame | 21 octets en Service Data (UUID 16 bits provisoire `0xFD5C`, voir limites) : version, étiquette HMAC de l'époque de 15 min, membre et sauts, heure de la position, puis latitude, longitude (24 bits), drapeaux, progression le long du plan chiffrés en AES-256-CTR, MAC HMAC-SHA-256 tronqué à 4 octets. |
| Émission | Deux jeux de publicité legacy non connectables, non scannables, INTERVAL_MEDIUM (250 ms), TX_POWER_HIGH : la trame du téléphone et, à tour de rôle, la dernière position connue d'un autre membre (un saut de plus). Données remplacées à chaque lot GNSS. |
| Écoute | Scan filtré dans le contrôleur sur le préfixe version + étiquette (époques précédente, courante, suivante), SCAN_MODE_LOW_POWER, résultats par lots (`setReportDelay` = délai du lot GNSS, 5 à 60 s) si le contrôleur sait les mettre en lot ; relancé seulement au changement d'époque (au plus 1 démarrage par 15 min). |
| Table | Dernière position de chaque membre avec son âge, sa source (directe ou relayée n fois) et ses drapeaux ; seule une position plus récente remplace l'ancienne (anti-rejeu), étiquettes d'époques anciennes refusées. |
| Alertes | Décrochage (non entendu depuis plus de 5 min et plus de 500 m derrière le long du plan commun, alors qu'il était derrière ; sans plan commun : à plus de 500 m) ; arrêt prolongé hors refuge (pause du suivi de plus de 10 min, pas au refuge) ; sortie de l'itinéraire (alerte du suivi, 40 m pendant 15 s). Jamais sur simple perte de contact. Dites et vibrées comme les alertes de sécurité du suivi. |
| Carte | Point par membre à sa dernière position (pâli au-delà de 2 min, rouge si arrêté ou hors itinéraire), prénom et âge (« Léa · 6 min »). |
| Écran | Menu « Carte et légende » → « Groupe » : créer, afficher le QR, scanner, membres (distance, âge, source, drapeaux), quitter. |

## Hors périmètre (non codé)

- **Relais réseau chiffré** : il faut un serveur relais (coût par utilisateur, à arbitrer avec S2) ; pas petit, donc non fait. SMS : dernier recours manuel du ticket K10.
- **Regroupement aux intersections du plan** et **SOS** : non codés (aucun drapeau SOS n'est émis).
- Publicité étendue (plusieurs membres par trame) : non, legacy seulement.

## Vérifier

```sh
cargo test -p skimap-group -p skimap-ffi
cd android
./gradlew :group:testDebugUnitTest :tracking:testDebugUnitTest :app:testDebugUnitTest --tests 'fr.skimap.app.GroupTest'
./gradlew :group:lintDebug :group:detekt :group:ktlintCheck
./gradlew :app:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=fr.skimap.app.GroupModeTest
```

## Limites

- Rien n'est mesuré sur téléphone : portée en poche, coût batterie, réception par lots écran éteint (§11 du rapport) restent à mesurer sur les deux Pixel.
- L'émulateur n'a pas de lien Bluetooth entre deux instances : le test instrumenté simule deux téléphones dans un processus, trames injectées de l'un à l'autre.
- UUID 16 bits non attribué au projet par le Bluetooth SIG (à régler avant publication).
- L'heure des trames vient de l'horloge du téléphone (GNSS en pratique) ; un téléphone très décalé (> 2 min dans le futur) voit ses positions refusées.
