# Comparatif des applis Android outdoor et cartographie pour le ski de fond et la rando nordique (Cervières, janvier 2027 ; Chapelle-des-Bois, février 2027)

Données relevées le 14/11/2026. Les notes, le nombre d'avis, les installations et les dates de mise à jour viennent des pages Google Play (`hl=fr&gl=FR`), téléchargées une par une.

**Limite importante sur les prix :** la page web du Play Store n'affiche plus de fourchette de prix pour les achats intégrés, seulement la mention « Achats via l'appli ». Les prix viennent donc de la description Play quand l'éditeur les y écrit, sinon de la page tarifs officielle. À défaut, j'ai pris la liste d'achats intégrés de l'App Store FR (iOS) : ces prix sont signalés **(iOS FR)** et le prix Android correspondant reste **non vérifié**.

## Ce qu'il faut retenir

- **Aucune appli généraliste n'affiche l'état de damage en temps réel pour Cervières ou Chapelle-des-Bois.** La seule qui affiche une date de dernier damage est Mapy.com, « sur certaines pistes », et la couverture française n'est pas confirmée.
- **Seuls OsmAnd, Locus Map et Mapy.com tracent un itinéraire sur les pistes de fond.** Les autres se contentent au mieux de les afficher.
- **Pour la Pixel Watch 2 (Wi-Fi seulement), quatre applis peuvent enregistrer sans le téléphone :**
  - komoot, nouvelle appli d'octobre 2026, avec cartes hors ligne et navigation sur la montre ;
  - SityTrail ;
  - Strava, qui enregistre sur la montre mais dont la carte sur la montre n'est pas vérifiée ;
  - Mapy.com, pour l'enregistrement seulement (la navigation exige le téléphone).

  WristNav est une petite appli tierce qui fonctionne entièrement sur la montre, cartes OSM hors ligne comprises.
- **FATMAP n'existe plus.** Strava l'a fermé le 1er octobre 2024, et fatmap.com redirige aujourd'hui (HTTP 301) vers strava.com.
- **IGNrando' est fermé depuis le 20/11/2025.** La même appli s'appelle désormais **WeTrek** (développée par Ubicarta). Cartes IGN est une appli différente, gratuite, lancée par l'IGN en 2024.

## Tableau de synthèse

Abréviations : HL = hors ligne ; P = payant ; G = gratuit.

| App (package) | Note Play / installs | Prix Android (EUR) | Hors ligne | IGN | Pistes de fond / couche hiver | Damage / neige | Enregistrement / GPX | Partage de position | Météo / BERA / pentes | Wear OS (Pixel Watch 2) |
|---|---|---|---|---|---|---|---|---|---|---|
| Visorando (org.visorando.android) | 4,7 (139 k) / 5 M+ | Premium 6 €/mois ou 25 €/an, essai 3 j | Tracés G ; cartes topo P | TOP25 1:25 000 en Premium | Non documenté | Hauteur de neige et neige fraîche (P) | Oui ; import GPX | Premium (réseau requis) | Météo, BERA, pentes >30° (P) | Aucune |
| Cartes IGN (fr.ign.geoportail) | 4,7 (5,56 k) / 1 M+ | 100 % gratuit | La plupart des fonds (certains exclus, lesquels non vérifié) | Plan IGN + cartes topo, G | Traces hivernales ski de rando/raquettes (800 dans les Alpes, Jura « prochainement ») | Non | Oui (depuis mai 2026) ; GPX import/export | « Partager sa position » (live ou ponctuel : non vérifié) | Non vérifié | Aucune |
| WeTrek, ex-IGNrando' (ubicarta.ignrando) | 4,0 (4,55 k) / 500 k+ | WeTrek Plus 22,99 €/an, essai 7 j | Plus uniquement | Plan IGN G en ligne ; TOP25 P | OpenSnowMap G | Non | Oui ; GPX/KML import/export | Oui (palier non vérifié) | Pentes >30° (P) | Aucune |
| Iphigénie (com.iphigenie) | 4,4 (9,33 k) / 1 M+ | Carto 24,99 €/an, essai 7 j | Carto (P) | IGN (P) | OpenSnowMap G | Non | Oui ; export GPX | Balise (beacon) | Pentes, Data-avalanche | Aucune |
| Decathlon Outdoor (com.decathlon.quechuafinder) | 4,8 (25,6 k) / 1 M+ | 100 % gratuit | Itinéraires G | Non (OSM) | Non (itinéraires raquettes présents) | Non | Oui ; export GPX non vérifié | Non mentionné | Non | Aucune |
| Outdooractive | 4,6 (75,1 k) / 5 M+, pubs | Basic G ; Pro 29,99 €/an ; Pro+ 59,99 €/an (annuel seulement) | Pro | « Cartes officielles » dès Pro (IGN citée dans la description Play) | Réseau hivernal fond/raquettes (couche Pro, source 2019) | Hauteur de neige Pro+ ; signalements G | Oui ; GPX G | BuddyBeacon (Pro) | Risque d'avalanche G ; pentes Pro ; météo Pro+ | Oui (enregistrer, naviguer) ; autonome non vérifié |
| komoot (de.komoot.android) | 4,6 (418 k) / 10 M+ | Premium 59,99 €/an ou 4,99 €/mois (iOS FR) ; Pack Monde 29,99 € | Région/pack ou Premium | Non | Ski de fond = sport enregistrable, pas planifiable | Non | Oui ; GPX | Live Tracking (Premium) | Météo (Premium) | **Appli autonome (oct. 2026)** : cartes HL, navigation, enregistrement ; paywall non vérifié |
| AllTrails | 4,4 (383 k) / 50 M+, pubs | Plus 35,99 €/an (web) ; iOS FR : Plus 23,99 €, Peak 53,99 € | Plus | **IGN Plan + Scan 25** (palier non indiqué) | Non | Hauteur de neige (Plus) | Oui | Live share (Plus) | Précipitations Plus ; pente Peak | Compagnon seulement, téléphone requis |
| Strava (com.strava) | 4,4 (1,2 M) / 100 M+ | 69,99 €/an ; Famille 119,99 € ; Étudiant 34,99 € ; essai 30 j | Itinéraires (abonnés) | Non | Style carte « hiver » ; pistes de fond non vérifié | Non | Oui (sport « Nordic Ski ») | Beacon G sur téléphone ; P sur montre | Pente, pente avalanche (abonnés, source secondaire) | **Enregistrement autonome** ; carte sur la montre non vérifiée |
| Gaia GPS | 3,9 (21,3 k) / 1 M+ | Premium 59,90 € (iOS FR) | Premium | France IGN (Premium) | Non vérifié | Non | Oui ; GPX/KML | **Aucun** (aide officielle) | Pentes (Premium) | Aucune trouvée |
| Locus Map 4 (menion.android.locus) | 4,8 (61,8 k) / 5 M+, pubs | Silver dès 10 €/an ; Gold dès 24 €/an | 3 LoMaps G ; toutes avec Gold | Via Locus Store (prix non vérifié) | **LoMap hiver avec pistes de fond + profil de calcul ski de fond** | Non | Oui ; nombreux formats | Live tracking (Premium) | Non vérifié | Locus Map Watch : carte sur la montre, **téléphone obligatoire** |
| OsmAnd (net.osmand) | 4,5 (238 k) / 10 M+ | 7 téléchargements G ; Maps+ 69,99 € à vie ou 14,99 €/an ; Pro 39,99 €/an | Oui | Sources raster perso (IGN non documenté) | **Style « Hiver et ski » + type de damage + calcul ski de fond** | Type de damage (classique/skating), pas l'état | Oui ; GPX | Non vérifié | Météo (Pro) ; relief (Maps+) | Aucune trouvée |
| Organic Maps (app.organicmaps) | 4,4 (23,5 k) / 5 M+ | 100 % gratuit | Oui, tout | Non | Non documenté | Non | Oui ; GPX/KML/GeoJSON | Non | Non | Aucune |
| SityTrail (com.geolives) | 4,5 (4,16 k) / 100 k+, pubs | Premium France 29,99 €/an ; Full 59,99 €/an | Premium | IGN topo + Plan IGN (Premium France) | Pistes de ski par couleur (P) ; fond non vérifié | Non | Oui ; export GPX/KML (P) | Fonction SOS (P) ; live non vérifié | Carte des pentes (P) | **Enregistrement autonome sur la montre** ; carte et paywall non vérifiés |
| Whympr (com.whympr.w) | 4,6 (1,4 k) / 100 k+ | Premium 29,99 €/an ; Pack Outdoor 39,99 €/an (+ Iphigénie Carto) | Premium | IGN (Premium) | Non mentionné | Non | Oui | Non vérifié | **BERA G** ; pentes G (FR/CH) ; météo P | Garmin/Suunto/Apple Watch, **pas Wear OS** |
| Mapy.com (cz.seznam.mapy) | 4,6 (248 k) / 10 M+, pubs | Premium 18,99 €/an (iOS FR), annuel seulement | Limité en G ; illimité en P | Non | **Carte hiver : types de pistes + calcul ski de fond mondial (G)** | « Dernier damage » sur certaines pistes (couverture FR non vérifiée) | Oui | **Gratuit** | Calque pentes (palier non vérifié) | Premium requis ; enregistrement autonome ; navigation via téléphone ; carte en ligne seulement |
| CalTopo | 4,5 (6,88 k) / 100 k+ | Non vérifié (~20 $ / 50 $ par an selon d'anciens billets) | Oui (P) | Non natif | Non | Non | Oui | Suivi live (cartes partagées) | Pentes, avalanche (orienté USA), météo | Aucune |
| Footpath | 4,6 (9,57 k) / 1 M+ | Elite 3,99 €/mois ou 24,99 €/an (iOS FR) | Elite | IGN France (Elite) | Non | Non | Oui ; GPX | Non | Pentes avalanche (Elite) | Apple Watch seulement |
| Wikiloc | 4,5 (159 k) / 10 M+ | 19,99 €/an ou 9,99 €/trimestre (iOS FR) | Premium | Non vérifié | Non | Non | Oui ; « 80 activités » | Suivi live (Premium) | Météo (Premium) | Appli « miroir », téléphone requis |
| Relive | 4,3 (370 k) / 10 M+ | Plus 38,99 €/an ou 6,99 €/mois (iOS FR) | Non | Non | Non | Non | Oui (vidéo 3D) | Non | Non | Pas de fiche Montre |
| Relief Maps (fr.reliefmaps.app) | 4,6 (9,31 k) / 500 k+ | Premium 29,99 €/an (+ formules à vie sur iOS) | Premium | Plan IGN G ; IGN scan P | « OSM hiver » / carte hiver (P) | Non | Oui ; **import/export GPX en Premium** | Live tracking (P) | Météo neige, pentes (P) | Suunto seulement |
| MaRando FFRando (com.cirkwi.marando) | 4,7 (1,88 k) / 500 k+ | Gratuit | Itinéraires G | Non vérifié | Non | Non | Oui ; téléchargement GPX | Non | Non | Aucune |
| AlpineQuest (psyberia.alpinequest.full) | 4,8 (9,96 k) / 100 k+ | **14,99 € achat unique** | Oui | Sources en ligne (IGN non vérifié) | Non | Non | Oui ; GPX/KML/… | Non | Pentes (MNT) | Aucune |
| WristNav (com.wristnav.app) | Note non affichée / 1 k+ | Gratuit (aucun achat intégré visible) | **Sur la montre** (OSM) | Non | Non | Non | Sur la montre ; GPX | Non | Non | **100 % autonome** |
| FATMAP | — | **Fermé le 01/10/2024** | — | — | — | — | — | — | — | — |

Suunto (4,2 / 1 M+) et Garmin Explore (3,9 / 1 M+) ne servent à rien sans montre de la marque, donc je ne les ai pas détaillés.

## Données OSM sur les deux sites (requête Overpass du 14/11/2026, 18h16 UTC)

Ces données alimentent OsmAnd, Locus, Mapy.com et la couche OpenSnowMap d'Iphigénie et WeTrek.

- **Chapelle-des-Bois** (zone 46.54–46.66 N / 6.02–6.20 E, qui déborde sur les domaines voisins) :
  - 557 tronçons `piste:type=nordic`, soit 256,6 km, et 79 relations (dont « GTJ à ski de fond », « La Citadelle », « Chalet Brulé ») ;
  - type de damage `classic;skating` sur 500 tronçons ;
  - 24,9 km de pistes de raquettes (`piste:type=hike`).
- **Cervières** (zone 44.80–44.92 N / 6.62–6.82 E) :
  - seulement 61 tronçons, soit 13,1 km, et 11 relations (La Pinatelle, Le Cancel, Le Gibera, Izoard, Les Fonts…) ;
  - 1,2 km de raquettes et 5,2 km de ski de rando.
  - Je n'ai pas vérifié si c'est complet par rapport au plan officiel des pistes. **Ce chiffre bas laisse penser que le domaine est mal cartographié dans OSM (non vérifié).**

## Fiches par appli

Toutes les fiches Play ont l'adresse `https://play.google.com/store/apps/details?id=<package>&hl=fr&gl=FR`, consultée le 14/11/2026.

**Visorando.** Mise à jour Play le 9/11/2026. La description Play indique 6 €/mois ou 25 €/an, avec un essai de 3 jours. Le Premium donne les cartes IGN 1:25 000 (Top 25) et le partage de position en temps réel. La page Premium ajoute la hauteur de neige et la neige fraîche (précision de 20 m dans les Alpes et le Jura), les prévisions de neige, le BERA et les pentes de plus de 30° ; le partage de position y « nécessite une connexion réseau ». Sur iOS FR, il existe aussi une offre « Cartes IGN pour 1 an » à 18,99 €. Points forts : neige, BERA et pentes réunis, à un prix bas. Point faible : aucune couche de pistes de fond documentée. Sources : https://www.visorando.com/premium.html, https://apps.apple.com/fr/app/id969793415

**Cartes IGN.** Mise à jour Play le 2/11/2026. Appli « 100% gratuite ». Elle a ajouté la vue 3D et l'enregistrement de parcours le 5 mai 2026. Le 21 mars 2026, 800 traces hivernales (ski de rando et raquettes) ont été ajoutées dans les Alpes ; pour les autres massifs, l'IGN annonce « prochainement ». L'import et l'export se font en GPX et JSON. Selon Presse-Citron, « certains fonds… ne peuvent pas être récupérés hors ligne ». Sources : https://www.ffrandonnee.fr/s-informer/actualites/le-territoire-en-3d-avec-l-appli-cartes-ign, https://www.ffrandonnee.fr/s-informer/actualites/la-randonnee-hivernale-plus-sure-dans-les-alpes-avec-l-appli-carte-ign, https://www.presse-citron.net/telecharger/geographie-et-voyage/cartographie-geolocalisation/cartes-ign.html

**WeTrek (ex-IGNrando').** Prix (22,99 €/an) et liste des cartes tirés de la description Play : Plan IGN et OpenSnowMap gratuits, TOP25 et pentes de plus de 30° avec l'abonnement Plus. Selon la FFRandonnée (article du 6/1/2026), IGNrando a fermé le 20/11/2025 et l'export des données était possible jusqu'au 20/5/2026. Journal du Geek (3/11/2026) la présente comme « la nouvelle version de IGNrando ». Sources : https://www.ffrandonnee.fr/s-informer/actualites/fermeture-de-l-application-ignrando-comment-transferer-vos-donnees-sur-marando, https://www.journaldugeek.com/telecharger/loisirs/geographie_et_voyage/ignrando.html

**Iphigénie.** Tout vient de la description Play : abonnement Carto à 24,99 €/an, 25 fonds gratuits dont Open Snow Map, et pour les abonnés l'IGN, le hors ligne, les pentes, Data-avalanche, la balise et l'export GPX. Iphigénie Carto est aussi inclus dans le Pack Outdoor de Whympr. Aucune appli montre.

**Decathlon Outdoor.** Entièrement gratuit, avec un fond OpenStreetMap et des courbes de niveau (description Play). On trouve des itinéraires raquettes, par exemple https://www.decathlon-outdoor.com/fr-fr/explore/france/le-sentier-du-bugeon-a-raquettes-61c47db602927. Pas de cartes IGN, pas de couche hiver.

**Outdooractive.** Le tableau des formules donne Pro à 29,99 €/an et Pro+ à 59,99 €/an. Il range ainsi les fonctions :
- **gratuit :** « Voir les pistes de ski… », « Risques d'avalanche », signalements en temps réel ;
- **Pro :** hors ligne, BuddyBeacon, cartes officielles, déclivité, carte Avalanche Terrain Hazard ;
- **Pro+ :** météo, épaisseur de neige.

Le réseau hivernal (pistes de fond et raquettes) est une couche réservée à Pro et Pro+, selon un billet de 2019. Côté montre, la description Play dit qu'on peut « enregistrer vos traces… naviguer ». Le fonctionnement sans téléphone et le palier requis ne sont pas vérifiés. Sources : https://www.outdooractive.com/fr/membership/plans.html, https://corporate.outdooractive.com/oa-blog-en/the-new-winter-sports-trail-network-on-outdooractive

**komoot.** Pack Monde à 29,99 € et « Pack Région Gratuit » sur la boutique web. Premium à 59,99 €/an ou 4,99 €/mois sur iOS FR. Depuis février 2025, les nouveaux utilisateurs ont besoin du Premium pour envoyer un itinéraire vers une montre ou un compteur (BikeRadar). La doc de l'API classe `nordic` et `snowshoe` en « tracking-only », donc pas de calcul d'itinéraire en ski de fond ni en raquettes. L'appli Wear OS du 21/10/2026 offre cartes hors ligne, navigation, recalcul et enregistrement ; le live tracking sur la montre est seulement annoncé. Sources : https://www.komoot.com/fr-fr/shop, https://static.komoot.de/doc/external-api/v007/sports.html, https://www.bikeradar.com/news/komoot-extends-its-phone-free-tech-to-wear-os-devices, https://www.bikeradar.com/news/new-komoot-users-send-to-devices

**AllTrails.** Les articles d'aide ont été lus via l'API Zendesk, car la page web renvoie une erreur 403. L'article « Map types » (mis à jour le 9/11/2026) liste « IGN Plan and IGN Scan 25 », la hauteur de neige en Plus et l'angle de pente en Peak. L'article Wear OS précise : « not a standalone app, so it requires your phone ». L'abonnement Plus est à 35,99 €/an via la page cadeau. Sources : https://support.alltrails.com/hc/en-us/articles/37228180990228, https://support.alltrails.com/hc/en-us/articles/37215507920148, https://support.alltrails.com/hc/en-us/articles/43589223010708, https://www.alltrails.com/fr/gift, https://apps.apple.com/fr/app/id405075943

**Strava.** Les prix viennent de https://www.strava.com/subscribe?hl=fr-FR. Pour Wear OS, l'aide dit « You can record activities on your watch without carrying your phone ». Beacon sur la montre est réservé aux abonnés, et une montre sans LTE doit rester connectée au téléphone. Le style de carte hiver, la couche de pente et la pente avalanche sont décrits par Snowbrains le 18/3/2025 (source secondaire). Pour la fermeture de FATMAP : TechCrunch du 26/6/2024, et la redirection vers strava.com vérifiée aujourd'hui. Que Beacon soit gratuit sur téléphone repose uniquement sur des articles de presse de 2021. Sources : https://support.strava.com/en-us/articles/17828383-how-do-i-use-the-strava-app-on-my-wear-os-watch, https://snowbrains.com/strava-launches-powerful-new-3d-mapping-engine-adds-avalanche-and-terrain-layers, https://techcrunch.com/2024/06/26/strava-to-shutter-3d-mapping-platform-fatmap-18-months-after-acquisition

**Gaia GPS.** Aide officielle lue via l'API Zendesk :
- en gratuit, uniquement Gaia Topo et pas de hors ligne ;
- le Premium inclut « France IGN » et l'angle de pente ;
- l'article de juin 2026 dit qu'il « cannot share your current location ».

Le prix (59,90 €) vient d'iOS FR. Sources : https://help.gaiagps.com/hc/en-us/articles/115003524547, https://help.gaiagps.com/hc/en-us/articles/360048679934, https://apps.apple.com/fr/app/id1201979492

**Locus Map.** Silver dès 10 €/an et Gold dès 24 €/an. Trois LoMaps sont gratuites, toutes avec Gold ; le thème Ski inclut les « cross country skiing trails » (doc modifiée le 24/9/2026). Le profil de calcul ski de fond date de la version 4.13 (décembre 2022). L'appli montre est un écran déporté : « stand-alone function is not possible » (page modifiée le 7/8/2026). Sources : https://www.locusmap.app/premium/, https://docs.locusmap.app/doku.php/manual:user_guide:maps_locusmaps, https://www.locusmap.app/what-new-features-does-the-latest-locus-map-4-13-bring/, https://docs.locusmap.app/doku.php/manual:user_guide:add-ons:wearforlocus

**OsmAnd.** Prix et limite de 7 téléchargements gratuits sur la page des achats. Le module ski documente l'option « Piste grooming » ; le profil Ski accepte les pistes de fond. On peut ajouter des sources raster via une URL de tuiles. Sources : https://osmand.net/docs/user/purchases/android, https://osmand.net/docs/user/plugins/ski-maps, https://www.osmand.net/docs/user/navigation/routing/ski-routing, https://osmand.net/docs/user/map/raster-maps

**Organic Maps.** Description Play : 100 % gratuit et hors ligne, formats GPX, KML et GeoJSON. Aucune couche hiver documentée. Selon le wiki OSM, « OpenStreetMap Carto does not render piste ways » (https://wiki.openstreetmap.org/wiki/Key:piste:type).

**SityTrail.** Premium France à 29,99 €/an, avec IGN topographique, Plan IGN, pentes et SOS. La description Play annonce l'enregistrement sur Wear OS « en mode autonome ». La version vectorielle SityTrail+ n'inclut pas l'IGN 1:25 000 en raster. Sources : https://shop.geolives.com/products/SITYTRAIL_PREMIUM_FRANCE.html, https://www.sitytrail.com/en/wiki/eaa5bc88-b88f-4449-b902-7b3d3ec53aaf/

**Whympr.** Premium à 29,99 €/an ; Pack Outdoor à 39,99 €/an, avec 7 jours gratuits. BERA et pentes France/Suisse sont dans l'offre gratuite. Côté montre, la description Play cite « Garmin, Suunto et Apple Watch ». Sources : https://get.whympr.com/prix, https://apps.apple.com/fr/app/id1131231365

**Mapy.com.** Prix de 18,99 €/an confirmé par le blog Seznam du 27/11/2025. L'aide carte hiver indique les types de pistes et « information about the last grooming… on selected tracks ». Le calcul « on skis (worldwide) » est gratuit. Pour la montre, l'aide dit que Premium est requis, que la navigation exige le téléphone appairé et que « The map is available online only ». Sources : https://help.mapy.com/map/map-types/, https://help.mapy.com/route-planning/tools/, https://help.mapy.com/smart-watches/, https://blog.seznam.cz/en/2025/11/mapy-com-premium-we-kept-our-promise-and-added-even-more/

**CalTopo, Footpath, Wikiloc, Relive.**
- CalTopo : fonctions tirées de la description Play ; prix non vérifié.
- Footpath : fonctions sur https://footpathapp.com/elite (où le prix est « unavailable ») et prix iOS FR.
- Wikiloc : prix iOS FR. L'article d'aide Wear OS est bloqué par Cloudflare ; le fonctionnement « miroir » vient du résumé de recherche et de https://www.elgrupoinformatico.com/apps/wikiloc-permite-seguir-rutas-wear (2023).
- Relive : appli de vidéo 3D, pas de navigation.

**Relief Maps, MaRando, AlpineQuest, WristNav.** Fonctions et prix tirés des descriptions Play. Prix d'AlpineQuest : bouton « Acheter pour 14,99 € ». WristNav : « Aucun téléphone ni réseau n'est requis », compatible Pixel Watch.

## Non vérifié

- Prix Android exacts quand seul le prix iOS FR est cité (komoot Premium, AllTrails Peak, Gaia, Mapy, Wikiloc, Footpath, Relive, Relief Maps).
- Paliers d'abonnement exigés par les applis Wear OS de komoot, Outdooractive et SityTrail.
- Affichage d'une carte sur la montre pour Strava et SityTrail.
- Affichage des pistes de fond dans Strava, Gaia, Cartes IGN (Plan IGN), Visorando et SityTrail.
- Couverture française du « dernier damage » de Mapy.com.
- Conditions de licence du SCAN 25 quand on l'ajoute soi-même dans OsmAnd, Locus ou AlpineQuest.
- Exhaustivité des pistes OSM de Cervières.

Fichiers de travail (pages Play et réponses Overpass) : `research/raw/play/`, `research/raw/osm/`
