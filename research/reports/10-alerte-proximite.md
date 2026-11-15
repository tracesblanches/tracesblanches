# 10 — Alerte et groupe par proximité, sans réseau (15/11/2026)

Légende : **[V]** vérifié dans une source primaire (liens et citations : `10-alerte-proximite-sources.md`, renvois P1…P38) ; **[D]** déduit de sources vérifiées ; **[E]** estimation non mesurée. Tickets : N1 #127, N2 #128, K10 #61, S2 #60, E1/rapport 09. Aucun téléphone branché : rien n'est mesuré sur l'appareil.

## À retenir
- **Le mode groupe (N2) est le seul usage qui ne dépend pas de la densité d'utilisateurs** : tous les membres ont l'app par construction. C'est lui qu'il faut faire, en BLE « publicité seule » dans la FGS `location` déjà prévue (rapport 09) [D].
- Portée attendue **téléphone en poche : de l'ordre de 5 à 50 m**, pas 100 m. Le corps coûte environ 15 à 20 dB [V P17] ; au calcul, cela fait 5 à 15 m quand le corps masque et 25 à 60 m dans un cas favorable [E]. **C'est la première chose à mesurer.**
- Coût batterie du mode groupe (publicité toutes les 250 ms, écoute filtrée par lots écran éteint) : **≈ 0,6 à 1,3 mA, soit 0,06 à 0,14 % sur 5 h** [E]. Le vrai danger, ce sont les réveils du processeur (+28,6 mA) si chaque trame est livrée une par une [V P16 ; D].
- Le Pixel 8 annonce la LE Coded PHY (« longue portée »), mais **elle n'est pas reçue** : « Long range support is claimed but the signals are never received » (test du 1/2024, Android 14) [V P2]. L'UWB n'existe que sur le 8 Pro [V P1]. Il n'y a donc pas d'UWB entre un 8 et un 8 Pro.
- Le relais d'un appel de détresse par des inconnus est **improbable là où il servirait** : hors piste, la fréquentation est faible et la part d'équipés minuscule (≈ 2 % de chances en 3 h, §6) [E]. Sur piste, des passants et les pisteurs sont déjà là [D].
- **Jamais de recherche de victime** : 457 kHz, DVA + pelle + sonde [V P33-P36]. En recherche DVA, il faut éteindre le téléphone (interférences jusqu'à 80-100 cm [V P33, P34]).

## 1. Mode groupe (N2), en premier
- **Création sans compte, au chalet** : un membre crée le groupe. Le QR contient une clé de groupe de 32 octets, un numéro par membre (8 à 16 au plus [E]) et l'empreinte du plan commun (#110). Les autres le scannent. Pas de serveur, pas d'identifiant durable [D].
- **Trame** : publicité legacy non connectable (ADV_NONCONN_IND, le choix de GAEN [V P21]) avec une Service Data UUID 16 bits, soit environ 27 octets utiles [D]. Contenu : version (1 octet), étiquette de groupe tournante (4 : HMAC(clé, époque), époque de 10 à 20 min comme GAEN [V P21]), membre et nombre de sauts (1), lat/lon sur 24 bits chacune (6 ; ≈ 2,4 m), horodatage (2), drapeaux (1 : arrêté, hors itinéraire, SOS, batterie), MAC tronqué (4). Les champs sont chiffrés en AES-CTR, nonce = étiquette, membre et horodatage [D]. Il reste environ 8 octets (altitude, précision).
- **Retransmission de proche en proche** : chaque téléphone publie sa propre trame et, dans un second jeu de publicité, fait tourner les **dernières positions connues des autres** avec leur âge et leur nombre de sauts. On ne garde que le plus récent par membre, ce qui évite l'anti-rejeu par compteur [D]. Une publicité étendue (plusieurs membres par trame) n'est possible que si `getLeMaximumAdvertisingDataLength()` est > 31 sur les deux Pixel : à vérifier [V P3].
- **Fréquence** : trame mise à jour à chaque lot GNSS (5 à 60 s, rapport 09) ; publicité INTERVAL_MEDIUM 250 ms. INTERVAL_LOW (100 ms) « should not be used for continuous background advertising » [V P4]. Écran éteint, le scan d'une app en FGS passe en SCREEN_OFF (512 ms toutes les 10,24 s) ou SCREEN_OFF_BALANCED (183/730 ms) [V P6]. À 250 ms, chaque fenêtre de 512 ms voit environ 2 trames : **latence ≤ 10 s** pour un membre à portée [D].
- **Groupe qui s'étire** : un écart de vitesse de 0,5 m/s fait 300 m en 10 min, bien au-delà de la portée [D]. Ce qui reste utile hors portée : **la dernière position connue de chacun, son âge et sa source** (directe, relayée à n sauts, réseau). Elle circule dès que deux membres se croisent, aux pauses, aux intersections et au refuge [D].
- **Alertes**, calculées sur le téléphone du membre concerné puis diffusées comme drapeaux : décrochage : non entendu depuis plus de T (5 min par défaut [E]) et dernière position à plus de D derrière le long du plan (500 m par défaut [E]) ; texte du type « X non entendu depuis 6 min, vu 450 m derrière à 10 h 42 ; attendre à la prochaine intersection ? » ; arrêt prolongé hors refuge : mode pause du rapport 09 au-delà de N min, loin d'un refuge ; sortie de l'itinéraire commun : règle des 40 m pendant 15 s déjà dans le noyau ; regroupement : à chaque intersection du plan, si la dernière position d'un membre est derrière. Hystérésis : une perte de contact seule ne déclenche rien, car le corps qui masque coupe le lien à 5 m [D].
- **Avec réseau** : messages courts chiffrés de bout en bout par un relais aveugle (S2), opportunistes quand la radio est déjà réveillée [D]. **Mais un serveur coûte par utilisateur**, ce qui touche la contrainte du projet. Le SMS reste un dernier recours manuel : SEND_SMS est restreint et chaque envoi demande un appui (K10).
- **Choix technique** : **publicité BLE seule avec scan filtré par lots : oui**. Elle est sans connexion, va de 1 vers N, et le contrôleur filtre et met en lot : il ne réveille le processeur que sur une correspondance [V P8] ; GATT : seulement à la demande, écran allumé (gros transferts). Berty décrit des connexions BLE Android instables [P24] ; Nearby Connections : hors ligne, environ 100 m, BT + BLE + Wi-Fi, Play services, orienté connexion ; rien n'est documenté sur l'arrière-plan ni la batterie [V P14]. Non pour l'écoute continue ; Wi-Fi Aware : présent sur le Pixel 8 [V P2], mais la grappe « consumes resources and battery », exige Wi-Fi **et** Localisation actifs, et les messages font environ 255 octets [V P13]. Non pour le continu ; Coded PHY : non sur Pixel 8 [V P2] ; UWB : 8 Pro seul [V P1, P15].
- **Risques** : fausse séparation (masque du corps), fausse sécurité (« je le vois sur la carte » alors que la position a 20 min), Économiseur de batterie du Pixel (rapport 09), clé de groupe divulguée (n'importe qui lit les positions), anti-rejeu dépendant de l'heure GNSS [D].

## 2. Technologies sans réseau entre téléphones Android
| Techno | Pixel 8 / 8 Pro | Portée publiée | Débit / taille | Pour skimap |
|---|---|---|---|---|
| BLE legacy 1M | BT 5.3 sur les deux [V P1] | SIG : « More than a kilometer. Less than a meter. » ; puces nRF52840 : 655 m dehors (0 dBm) [V P18, P19] ; entre téléphones : non trouvé | 31 octets par trame | oui |
| BLE étendue / Coded | Coded annoncée mais non reçue sur Pixel 6, 7 et 8 ; reçue sur Pixel 9 (Android 16) [V P2] | nRF52840 en Coded : 1300 m [V P18] | jusqu'à 1650 octets (étendue) | à tester (étendue 1M) ; Coded non |
| Wi-Fi Aware (NAN) | reçu sur Pixel 8 [V P2] | « longer distances than Bluetooth », sans chiffre [V P13] | élevé (socket IPv6) | refuge, écran allumé |
| Wi-Fi Direct | non évalué en détail | — | élevé | non |
| Nearby Connections | Play services [V P14] | « ~100m » [V P14] | CLUSTER : « lower bandwidth » [V P14] | refuge, écran allumé |
| UWB | 8 Pro seul [V P1] | non publiée ; précision 10 cm, OOB par BLE requis [V P15] | — | non |

## 3. Arrière-plan, écran éteint, règles Android 15-17 et Play
- Un scan **non filtré** est refusé écran éteint (« Cannot start unfiltered scan in screen-off ») [V P6]. Il faut un ScanFilter (UUID 16 bits) [D].
- Une app à l'importance FOREGROUND_SERVICE ou mieux compte comme « foreground » pour le scan [V P6]. Notre FGS `location` suffit donc, sans type `connectedDevice` [D]. Si on l'ajoute, le prérequis est BLUETOOTH_SCAN, ADVERTISE ou CONNECT [V P11].
- Limites : **5 démarrages de scan par 30 s** (SCAN_FAILED_SCANNING_TOO_FREQUENTLY) ; **10 min** de scan continu, puis rétrogradation en LOW_POWER (SCREEN_OFF écran éteint), sauf pour CALLBACK_TYPE_FIRST_MATCH [V P6, P7]. On peut aussi utiliser `startScan(PendingIntent)` [V P10].
- Permissions : BLUETOOTH_SCAN, BLUETOOTH_ADVERTISE et BLUETOOTH_CONNECT sont des permissions d'exécution (« Nearby devices ») [V P9]. Avec `neverForLocation`, certaines balises sont filtrées [V P9]. Cela ne change rien pour notre UUID : l'app a déjà la localisation fine (09) [D]. Une app ne peut plus couper le Bluetooth (cible ≥ 33) [V P3].
- Android 16 et 17 : changements sur la perte d'appairage, la lecture RFCOMM (−1) et ACCESS_LOCAL_NETWORK (cible 37) ; **aucune nouvelle limite sur le scan ou la publicité BLE trouvée** [V P12]. Play : types de FGS à déclarer dans la Console [V P11].

## 4. Batterie (Pixel 8 : 1 %/h = 45,8 mA [V P16])
- Le profil AOSP du Pixel 8 **n'a aucune valeur Bluetooth** (Wi-Fi seul : 38, 57 et 550 mA) [V P16]. Aucune mesure publiée sur téléphone n'a été lue (l'étude SMU est inaccessible).
- Hypothèse : réception BLE à 10-20 mA pendant la fenêtre [E]. Écoute : SCREEN_OFF (5 %) ≈ 0,5 à 1 mA ; SCREEN_OFF_BALANCED (25 %) ≈ 2,5 à 5 mA ; LOW_LATENCY ≈ 10 à 20 mA [V cycles P6 ; mA E]. Publicité à 250 ms ≈ 0,06 à 0,25 mA [E].
- Sortie de 5 h (écoute SCREEN_OFF, publicité à 250 ms, lots alignés sur le GNSS) : 3 à 6 mAh, soit **0,06 à 0,14 %** ; en BALANCED, 0,3 à 0,6 % ; en LOW_LATENCY, 1 à 2 % ; **+3 %** si le processeur se réveille sans arrêt [E]. C'est à comparer aux 4 à 9 % de la sortie complète (09).

## 5. Réseaux de relais existants
- **Bridgefy** : cassé deux fois. En 2021, « participants of protests should avoid relying on Bridgefy » ; en 2022, après passage à libsignal, une attaque réussit ≈ 50 % des fois et le suivi des utilisateurs reste possible [V P22]. Amigo (CCS 2025) : les maillages antérieurs échouent face à la mobilité, au turnover des liens et à l'encombrement radio [P22, résumé].
- **Briar** : synchronisation par Bluetooth, Wi-Fi ou carte mémoire, Mailbox pour le différé, entre contacts [V P23]. **Berty** : Android Nearby, Multipeer et BLE ; « not yet … sensitive data » [P24, résumé]. **Bitchat** (2025) : critiques de Trail of Bits, empoisonnement du cache du stockage-retransmission corrigé le 28/01/2026 [P25, secondaire].
- **FireChat** : utilisé à Hong Kong en 2014, abandonné vers 2018 [P26, secondaire]. **Meshtastic** : LoRa, matériel externe, record de 331 km au sol [P26, résumé]. goTenna et les projets DTN : non lus.
- **Find Hub** de Google (option « everywhere » pour les zones isolées ; un Pixel 8 éteint reste localisable plusieurs heures) [V P27]. Ce réseau à densité de milliards de téléphones n'informe que le propriétaire, pas les secours [D].

## 6. Probabilité de croiser un autre utilisateur [E sauf mention]
- Nordic France 2025-26 : **2,5 M journées skieurs** payantes, 160 domaines, 169 jours [V P28]. Cela fait ≈ 92 par domaine et par jour en moyenne, très concentrés sur les week-ends et les vacances [D]. Aucun comptage sur les itinéraires de SRN n'a été trouvé.
- Modèle : victime immobile, flux F de passants par heure, part d'équipés p, détection q ≈ 0,8 → P = 1 − exp(−pqFT). Piste fréquentée, F = 30/h, p = 1 %, T = 1 h : **21 %**. SRN, F = 0,2/h, p = 5 %, T = 3 h : **2 %**.
 Au refuge (19 autres personnes) : au moins un autre équipé vaut 1 − (1 − p)^19, soit **17 % à p = 1 %** et 62 % à p = 5 %.

## 7. Sécurité, 112, vie privée, abus
- Une app ne peut pas appeler le 112 : « this Intent cannot be used to call emergency numbers » ; ACTION_DIAL le peut [V P30]. Le relayeur voit donc la détresse et appelle lui-même [D]. Le 112 est gratuit, depuis un fixe ou un portable ; le SMS d'urgence n'existe qu'au 114 (personnes sourdes ou aphasiques) [V P29]. AML est actif en France depuis 2020 ; pas de SOS par satellite sur Pixel 8 [V P31].
- Fausses alertes : « Les fausses alertes se multiplient », constatent les gendarmes de haute montagne, à cause des détections de chute automatiques [V P32]. **Aucun envoi automatique** : SOS par appui long avec compte à rebours, toujours marqué « non vérifié » chez le relayeur [D].
- RGPD : la localisation est une donnée personnelle [V P38]. D'où les étiquettes tournantes, l'adresse BLE aléatoire tournée en même temps (modèle GAEN [V P21]), le chiffrement et le MAC par groupe, l'anti-rejeu par horodatage GNSS, et le SOS public signé (Ed25519, publicité étendue ou GATT) [D].
- Abus : un faux SOS diffusé par n'importe qui reste invérifiable. Il faut une limitation et le libellé « signal reçu d'un inconnu, peut être faux » [D].

## 8. DVA
- Règle 20/50 (20 cm du DVA en émission, 50 cm en recherche) : Genswein, Atkins (vice-président de la commission avalanche ICAR) et al., ISSW 2013 [V P33]. Un téléphone à 40 cm brouille comme un enseveli à 50 m ; le brouillage ne passe sous le seuil qu'à 80 cm [V P34]. Edgerly (ISSW 2023) : rien au-delà de 10 cm, et le mode avion ne change rien [V P35]. ICAR 2006 : DVA, sonde et pelle ont fait leurs preuves, le reste relève de l'« opinion » [V P36]. Le CAC a déconseillé les « apps DVA » en 2013 [P37, presse].
- Formulation : « traceblanche n'est pas un DVA et ne retrouve personne sous la neige. En terrain avalancheux : DVA, pelle, sonde. Téléphone à plus de 20 cm du DVA en émission ; en recherche, éteignez-le. »

## 9. Montre
- Pixel Watch 2 : Bluetooth 5.0, Wi-Fi 2,4 GHz, 306 mAh, pas d'UWB indiqué [V P1]. Elle est au poignet, à côté du téléphone, donc aucun gain de relais [D]. 1 à 3 mA d'écoute y coûteraient 0,3 à 1 %/h [E]. Elle sert seulement à afficher et faire vibrer les alertes du téléphone.

## 10. Usages classés (valeur × faisabilité)
| Usage | Techno | Portée | Batterie 5 h | Succès si | Risques | Verdict |
|---|---|---|---|---|---|---|
| 1. Mode groupe (N2) | publicité BLE + scan filtré en lots | 5-50 m en poche [E] | 0,1-0,6 % [E] | aucune densité requise | fausse séparation, fausse sécurité | **faire** |
| 2. Balise SOS BLE + 112 par le passant | même trame, drapeau SOS, signée | idem | ≈ 0 en plus | p × F élevé (piste) | faux SOS | faire **seulement** comme option du 1 |
| 3. Signalements au refuge | Nearby / GATT, écran allumé | pièce | négligeable | ≥ 2 équipés présents (17 % à p = 1 %) | spam | v2 plus tard |
| 4. Maillage général entre inconnus | — | — | — | densité inexistante | Bridgefy | **ne pas faire** |

## 11. Prototype mesurable, deux Pixel (cas réel : dehors, en poche, froid)
1. **Capacités** (1 h) : journaliser `isLeCodedPhySupported`, `isLeExtendedAdvertisingSupported`, `getLeMaximumAdvertisingDataLength`, `isOffloadedFilteringSupported`, `isOffloadedScanBatchingSupported`, `FEATURE_WIFI_AWARE` et `android.hardware.uwb` sur le 8, le 8 Pro et la montre ; relever `dumpsys bluetooth_manager` (mode de scan effectif écran éteint, après 10 min) [V P3, P6].
2. **Portée** : terrain neigeux ouvert, puis forêt de résineux ; 5, 10, 20, 40, 80 et 160 m (GNSS et décamètre) ; 2 min par point. Configurations : deux téléphones en main ; deux en poche intérieure ; poche, dos tourné ; haut du sac.
 Paramètres : TX HIGH puis MAX_AVAILABLE ; 1M legacy, 1M étendue, Coded. Mesure : **portée où P(≥ 1 trame en 10 s) ≥ 95 %**, et RSSI. Puis séparation en marche, avec la distance de dernière réception et celle de reprise.
3. **Batterie** : deux sorties de 3 à 5 h identiques (A : FGS `location` seule ; B : A + mode groupe), Perfetto (`android.power`, compteurs de charge). Bancs de 30 min écran éteint pour SCREEN_OFF, BALANCED et LOW_LATENCY. On rapporte le **% de toute la sortie**. Test `dumpsys deviceidle force-idle` : la réception continue-t-elle ?
4. **Scénario N2** : séparation simulée de 600 m, arrêt de 15 min, sortie d'itinéraire ; mesurer le délai et le texte de l'alerte. Critères : portée en poche ≥ 20 m, delta ≤ 1 % sur 5 h, zéro fausse alerte de décrochage sur 1 h côte à côte.

## Recommandation
**Faire le mode groupe N2 en BLE (publicité seule, scan filtré en lots, dans la FGS `location`), avec la balise SOS comme simple drapeau de la même trame. Ne pas faire de maillage général, ni Wi-Fi Aware ou Nearby en continu, ni Coded PHY, ni UWB.** La décision finale tombe après l'étape 2 : si la portée en poche est inférieure à 10 m, le mode groupe se limite aux regroupements et au réseau.

## Non vérifié
Portée et consommation BLE mesurées entre téléphones (aucune publication trouvée) ; sensibilité de réception du Pixel 8 ; réception de la publicité étendue 1M sur Pixel 8 ; Wi-Fi Aware sur Pixel Watch 2 ; comptages SRN ; coefficients du modèle de Weissberger (tableau en image dans P20) ; textes de Briar (relais entre contacts), Berty, Bitchat et Meshtastic, lus seulement en résumé ; cases EENA pour la France.
