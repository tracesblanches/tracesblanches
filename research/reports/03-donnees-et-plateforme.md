# Ski de fond et rando nordique (Cervières, Chapelle-des-Bois) : briques de données, plateforme, licences et coûts

État au 2026-11-14.

**En bref :**
- **Fonds de carte :** le Plan IGN v2 (raster et vectoriel) est gratuit, sans clé et sous Etalab 2.0. Le SCAN 25 Touristique demande une clé, et sa mise en cache hors-ligne coûte 6 € HT par utilisateur et par an.
- **Tracés des pistes :** OSM est déjà très bien rempli sur les deux sites. Pour le Jura, il existe en plus une API ENJ publique qui fournit des GPX.
- **État des pistes en direct :** je n'ai trouvé aucune API ouverte (Nordic France, Apidae, Lumiplan, Snowsat). Il faudra un partenariat.
- **Montre :** l'API Health Services propose le type `CROSS_COUNTRY_SKIING`. Health Connect ne l'a pas : il n'y a que `SKIING`.

## 1. Fonds de carte

**Ce que j'ai testé moi-même sur la Géoplateforme** (tuile z15 au-dessus de Cervières) :
```
GetCapabilities sur data.geopf.fr/wmts            -> 200 (3,1 Mo)
Plan IGN v2 (GEOGRAPHICALGRIDSYSTEMS.PLANIGNV2), sans clé  -> 200 image/png
SCAN25TOUR sur /private/wmts, sans clé             -> 401 Unauthorized
SCAN25TOUR sur /private/wmts avec apikey=ign_scan_ws -> 200 image/jpeg
Tuile vectorielle PLAN.IGN (tms/1.0.0, z14, .pbf)  -> 200 application/x-protobuf
Style vectoriel PLAN.IGN/standard.json              -> 200
```
Le SCAN 25 et le SCAN 25 Touristique ne figurent pas dans le WMTS public ; ils ne sont servis que sur `private/wmts`.

| Source | Accès | Licence et limites | Hors-ligne |
|---|---|---|---|
| **Plan IGN v2** (raster et vectoriel, zoom 0 à 18/19) | Sans clé | Etalab 2.0 : l'article 1 de la [décision IGN 2021-295](https://data.geopf.fr/annexes/ressources/documentation/bareme_scans.pdf) place les « données éditées par l'IGN sans droits de tiers » en licence ouverte. D'après les [CGU de cartes.gouv.fr](https://cartes.gouv.fr/cgu/), les tuiles vectorielles TMS sont limitées à 400 requêtes/s par IP. Le WMTS ne figure pas dans le tableau des limites. Au-delà, le service renvoie une erreur 429 pendant 5 s. | La licence Etalab l'autorise. Les CGU n'interdisent pas explicitement le préchargement, mais ne l'autorisent pas non plus en termes exprès (**non vérifié** au-delà de cette lecture). |
| **SCAN 25 / SCAN 25 Touristique** | Clé obligatoire, créée sur cartes.gouv.fr avec un SIRET ([tutoriel](https://cartes.gouv.fr/aide/fr/partenaires/ign/representations-cartographiques-souveraines/creation-cles-donnees-scan/introduction/)). Ces données « ne sont pas libres de droit ». | Une app grand public relève d'une « Licence Usage Numérique Grand Public » ([conditions de licence](https://data.geopf.fr/annexes/ressources/documentation/conditions-de-licence.pdf)). Barème de l'annexe 2021-295 : streaming seul gratuit sous 10 M transactions par an (1 transaction = 16 tuiles WMTS), puis 5 000 € HT par tranche de 10 M. | Le forfait « streaming seul » **interdit** le téléchargement et la mise en cache par l'utilisateur. Le forfait « streaming + mise en cache » coûte **6 € HT par utilisateur final et par an**. Le forfait « préchargé » coûte 5 € HT par téléchargement si l'emprise est inférieure à un département. |
| **OpenTopoMap** | Sans clé (tuile testée : 200) | CC-BY-SA, usage commercial permis. Pas de « Massendownloads », pas de garantie de disponibilité, serveur passé sur un petit vServer le 05/01/2026 ([about](https://opentopomap.org/about)). | Préchargement massif déconseillé. Mieux vaut générer ses propres tuiles. |
| **OpenSnowMap** | Calque pistes `tiles.opensnowmap.org/pistes/{z}/{x}/{y}.png` (testé : 200) | Données ODbL, tuiles CC-BY-SA. « Bulk download of tiles by any application is strictly forbidden without prior acknowledgement » ([data.html](https://www.opensnowmap.org/iframes/data.html)). | Des paquets MBTiles sont prévus pour ça : `world_pistes_only.mbt` (5 Go) et `france_pistes_only.mbt` (509 Mo, « on request »). |
| **MapTiler** | Clé | Free : 0 $, 5k sessions, usage non commercial. Flex : 30 $/mois, 25k sessions ([pricing](https://www.maptiler.com/cloud/pricing/)). Les CGU limitent le cache à un « temporary personal cache » pour un seul utilisateur ([terms](https://www.maptiler.com/cloud/terms/)). | Pack hors-ligne par plan : **non vérifié** |
| **Thunderforest (Outdoors)** | Clé | Hobby gratuit (150k tuiles/mois), Solo 115 €/mois, Small Business 235 €/mois ([pricing](https://www.thunderforest.com/pricing/)) | Le bulk download (préparation hors-ligne) n'est permis qu'à partir de Small Business. |
| **Stadia** | Clé | Free (200k crédits) non commercial, Starter 20 $/mois, Standard 80 $/mois ([pricing](https://stadiamaps.com/pricing)) | Seulement en Enterprise (« offline media use cases ») |
| **Mapbox** | Token | 25 000 MAU gratuits, puis 4 $ par tranche de 1 000 MAU ([pricing](https://www.mapbox.com/pricing)) | Limité à 750 tile packs. Les données ne peuvent être ni préchargées, ni intégrées, ni redistribuées ([doc offline](https://docs.mapbox.com/android/maps/guides/offline/concepts/)). |
| **Protomaps (PMTiles OSM)** | Builds quotidiens sur maps.protomaps.com/builds | ODbL « Produced Work », attribution OSM obligatoire ([docs](https://docs.protomaps.com/basemaps/downloads)) | Auto-hébergement libre. Une extraction régionale se fait avec `pmtiles extract`. Le planet complet fait environ 120 Go. |

La clé `ign_scan_ws` circule sur des forums (par exemple [utagawavtt](https://www.utagawavtt.com/forum_v3/viewtopic.php?p=126162)) et elle fonctionne bien techniquement. Je n'ai trouvé aucune source qui en fasse une clé utilisable par un tiers. L'utiliser dans une app sans licence IGN ne serait pas conforme : **ne pas l'utiliser**.

## 2. Données de pistes

**Tags OSM**
- Valeurs `piste:grooming` pour le nordique ([wiki](https://wiki.openstreetmap.org/wiki/Key:piste:grooming)) : `classic`, `skating`, `classic+skating` (ou `classic;skating`), `scooter`, `backcountry`.
- Pour `piste:type=hike` : `classic` (damé, à pied) ou `backcountry` (raquettes).
- `piste:type=snowshoe` est classé comme une erreur ; on utilise `hike` ([wiki](https://wiki.openstreetmap.org/wiki/Key:piste:type)).
- `piste:difficulty` : `novice`, `easy`, `intermediate`, `advanced`, etc.
- `piste:status=open|closed` existe, mais le wiki le déconseille si la donnée n'est pas tenue à jour ([Piste_Maps](https://wiki.openstreetmap.org/wiki/Piste_Maps)). OSM décrit donc la nature du damage, **pas son état du jour**.

**Ce que contient OSM sur les deux sites** (requête Overpass, comptage des tags) :
```
Cervières (bbox 44.80,6.62,44.92,6.85), extrait :
  19 way|nordic|classic;skating|intermediate
  14 way|nordic|classic;skating|novice
   9 way|nordic|classic;skating|easy
   7 way|nordic;hike|classic;skating|advanced
   6 way|nordic|classic;skating|advanced
   5 relation|nordic
   3 way|hike|classic|easy  ...
  12 relations nommées : La Pinatelle (noire), Le Cancel, Le Gibera, Le Savare, Izoard,
     Route du Col d'Izoard, La Bergerie, Les Clots, Les Pins, L'Aigalan,
     Le Chemin du Sioux, Liaison Le Laus - Les Fonts
Chapelle-des-Bois (bbox 46.55,6.03,46.66,6.18), extrait :
  302 way|nordic|classic;skating|easy
   89 way|nordic|classic;skating|intermediate
   64 relation|nordic
   37 way|nordic|classic;skating|novice
   18 way|hike|backcountry ...
```
- Le cadre de Cervières déborde sur le domaine alpin voisin (Briançon et alentours) : les pistes `downhill` comptées viennent de là.
- Le détail des relations de Chapelle-des-Bois n'a pas pu être listé : les serveurs Overpass (principal et miroirs) étaient saturés (502).
- Pour une app, mieux vaut ne pas dépendre de l'Overpass public. OpenSnowMap publie un extrait mondial quotidien `planet_pistes.osm.gz` (204 Mo, état `2026-11-13T23:00:00Z`, vérifié).

**Autres sources**

| Source | Contenu pour nos deux sites | Accès |
|---|---|---|
| **ENJ Snowmap (Geotrek-admin)** | C'est la vraie trouvaille. `https://admin.snowmap.espacenordiquejurassien.com/api/v2/trek/` répond **sans authentification** : 641 itinéraires, dont **42 pour Chapelle-des-Bois** (`cities=14`). Chacun a une géométrie LineString, une difficulté, une longueur et des exports GPX/KML (GPX testé : 200). Les pratiques sont : 1 Raquettes, 2 Ski de fond, 3 Multi-activités, 4 Chiens de traîneau. | Gratuit en pratique. **Licence non affichée, non vérifiée** : demander à ENJ avant tout usage. Pas de champ d'état ou de damage. |
| **data.gouv.fr** | Recherches « ski de fond », « pistes nordiques », « domaine nordique » : 0 résultat. On trouve des extraits OSM de la Région PACA (ODbL), un jeu Pyrénées, et rien pour les Hautes-Alpes, le Doubs ou le Jura. | — |
| **DATAtourisme** (Licence Ouverte 2.0, CSV quotidiens) | La classe `CrossCountrySkiTrail` existe mais n'apparaît que **3 fois** dans les 27 595 itinéraires du CSV national. Chapelle-des-Bois : 21 itinéraires (Doubs Tourisme, surtout pédestres). Cervières : 0 itinéraire, mais 26 points d'intérêt venus d'Apidae (« Foyer de ski de fond », etc.). Les CSV ne donnent que des points, pas de tracé. | [data.gouv](https://www.data.gouv.fr/datasets/datatourisme-la-plateforme-nationale-des-donnees-touristiques). L'API demande une clé gratuite. |
| **Apidae** | Types d'objets `DOMAINE_SKIABLE` (`informationsDomaineSkiable`) et `EQUIPEMENT` (« Piste de ski de fond », « Itinéraires ski de fond ») ([types](https://dev.apidae-tourisme.com/documentation-technique/formats-des-objets/types-dobjet-touristique/), [tuto Savoie](https://www.savoie.fr/upload/docs/application/pdf/2022-09/tuto_ou_saisir_loffre_nordique_v1.1-apidae_asmb_12-21.pdf)). Le champ `ouverture` contient des périodes, pas un état du jour. Je n'ai trouvé aucun champ d'enneigement documenté. | Compte « réservé aux clients de l'offre Connect », créé par un administrateur du réseau ([créer son compte](https://dev.apidae-tourisme.com/documentation-technique/premiers-pas/creer-son-compte/)). Tarif **non publié**. |
| **SkiRadar** | Flux gratuit sans compte (`api.skiradar.fr/api/v1/geo-feed`, 30 requêtes/min) : 315 stations, dont 171 ont un bloc nordique. **Ni Cervières ni Chapelle-des-Bois n'y figurent** (vérifié sur les deux pages du flux). | Réutilisation avec attribution « skiradar.fr » ([data.gouv](https://www.data.gouv.fr/dataservices/skiradar-feed-json-des-stations-de-ski-francaises)) |

## 3. Damage et état des pistes en direct

| Système | Ce qui existe | Ouvert ? |
|---|---|---|
| **Nordic France** (bulletin en marque blanche) | La page `nordicfrance.fr/marqueblanche/station/4774` (Chapelle-des-Bois) affiche l'état **piste par piste** : « Dernier damage : 25/02 », « Mise à jour : 08/03 », « PISTES 0 / 27 », « Fermée »… | **Aucune API publique.** Le `wp-json` ne contient que des espaces de noms WordPress standard. Seul le HTML serveur est disponible, donc il faut un **partenariat** (ne pas scraper sans accord). Pour Cervières, la page a expiré en timeout : **non vérifié**. |
| **ENJ Snowmap** | Renvoie vers le bulletin Nordic France ci-dessus | L'API Geotrek ne donne pas d'état. |
| **Apidae** | Périodes d'ouverture seulement | Offre Connect payante |
| **Lumiplan, Skiinfo** | Aucune doc développeur trouvée | **Non vérifié** : partenariat présumé |
| **SNOWsat (PistenBully)** | Suivi de flotte et carte de hauteur de neige LiDAR, en interne à la station ([snowsat FAQ](https://www.snowsat.com/en/faq)) | Aucune API tierce trouvée |
| **Skisporet (Norvège)** | Damage alimenté par le GPS d'environ 1 400 dameuses | Pas d'API publique. Un service de station ([Kvitfjell](https://www.alpinco.com/en/kvitfjell/new-and-free-solution-trail-information)) dit qu'il est devenu payant puis qu'il ferme (**non vérifié**). |
| **Pyreneige** (modèle à imiter) | API JSON publique de bulletins, Pyrénées uniquement ([API](https://www.pyreneige.fr/ressources/api/)) | Ouverte (hors zone) |

## 4. Météo et avalanche

| Source | Faits |
|---|---|
| **BERA (Météo-France)** | 36 bulletins (Alpes, Corse, Pyrénées), en XML plus 6 PNG, de novembre à fin mai vers 16 h, via l'API « DonneesPubliquesBRA » avec authentification ([spécification XML](https://donneespubliques.meteofrance.fr/client/document/descriptif_bulletinrisqueavalanche_formatxml_20231026_309.pdf)). Sans token, `public-api.meteofrance.fr/public/DPBRA/v1/...` répond **401**. Le massif de Cervières est probablement **Queyras (17)**, à côté de Thabor (13) et Pelvoux (16) : rattachement **non vérifié**. **Le Jura n'a pas de BERA** (absent de la liste des massifs). |
| **API Météo-France** | « API Bulletin Avalanche », « Données d'observation » (6 min ou horaire, plus de 2 000 stations), « Climatologie », AROME et ARPEGE : toutes « Ouvert avec un compte » ([liste data.gouv](https://www.data.gouv.fr/organizations/meteo-france/dataservices)). Quotas officiels du portail : **non vérifiés**. |
| **Simulation nivologique** | CSV quotidiens sur les principaux massifs, Licence Ouverte 2.0 ([jeu](https://www.data.gouv.fr/datasets/donnees-de-modele-de-simulation-nivologique)). Paramètres et couverture du Jura : **non vérifiés**. |
| **Stations Nivôse** | Je n'ai pas trouvé de produit open data dédié. Meteociel affiche les capteurs de neige ([page](https://www.meteociel.fr/obs/neige_nivoses.php)). **Non vérifié.** |
| **Open-Meteo** | Testé : `snow_depth` et `snowfall` fonctionnent avec le modèle par défaut (Cervières : altitude 1698 m, 0,0 m ; Chapelle : 1092 m, 0,0). Avec `models=meteofrance_seamless`, `snow_depth` vaut **null**. L'offre gratuite est **non commerciale** (10 000 appels/jour). Données CC BY 4.0. Offres Standard / Professional / Enterprise, prix non affichés sur la page ([pricing](https://open-meteo.com/en/pricing)). |

## 5. Plateforme Android et Wear OS

**Montre (Pixel Watch 2)**
- Wear OS 4 à la sortie, 32 Go de stockage, 2 Go de RAM, GPS + Galileo + GLONASS + BeiDou ([specs](https://support.google.com/googlepixelwatch/answer/12651869?hl=en-IN)).
- Passée à Wear OS 6 (Android 16) le 8/10/2025 ([9to5google](https://9to5google.com/2025/10/08/pixel-watch-wear-os-6-update/)).
- Autonomie officielle : 24 h écran toujours allumé. L'autonomie GPS (environ 10 h) ne vient que de tests de presse ; la page Google ne la donne pas : **non vérifié**.
- La version Wi-Fi n'a pas de LTE : les cartes doivent être **stockées sur la montre**.

| Sujet | Constat |
|---|---|
| **Health Services** | `ExerciseType` contient bien `CROSS_COUNTRY_SKIING`, `SKIING`, `ALPINE_SKIING`, `SNOWSHOEING`, `BACKPACKING` et `HIKING` (vérifié dans la [référence](https://developer.android.com/reference/androidx/health/services/client/data/ExerciseType)). Le support dépend de l'appareil : il faut appeler `getCapabilitiesAsync()`, utiliser un `ForegroundService` de type `health\|location` et `isGpsEnabled=true`. Écran éteint, les données arrivent par lots, et `BatchingMode` « impacte la batterie » ([guide](https://developer.android.com/health-and-fitness/guides/health-services/active-data)). Le support réel de `CROSS_COUNTRY_SKIING` sur la Pixel Watch 2 est **à tester sur l'appareil**. |
| **Health Connect** | **Pas de type ski de fond** : `EXERCISE_TYPE_SKIING`=61, `SNOWSHOEING`=63, `HIKING`=37. Le tracé GPS passe par `ExerciseRoute` ([référence](https://developer.android.com/reference/kotlin/androidx/health/connect/client/records/ExerciseSessionRecord)). |
| **Google Maps sur Wear** | Supporté (mode ambiant simplifié), mais « Tile overlays are **not supported** on wearable devices » ([doc](https://developers.google.com/maps/documentation/android/wear?hl=en)). Donc pas de calque IGN ni de calque pistes. Hors-ligne : non documenté. |
| **MapLibre Native Android** (BSD-2-Clause, actif) | Supporte les PMTiles (PR #2882), y compris depuis `assets/` (`pmtiles://asset://`, #4602), et les MBTiles (`MBTilesFileSource`) ; c'est dans le CHANGELOG. **Sur Wear OS : aucune preuve trouvée**, à prototyper. |
| **Autres SDK** | **osmdroid** est archivé sur GitHub (Apache-2.0) : à éviter. **Mapsforge** et **VTM** sont en LGPL-3.0, actifs (oct. 2026), légers et hors-ligne natifs. Carto Mobile SDK : **non vérifié**. |
| **Formats** | PMTiles (spécification CC0, implémentations BSD-3, une seule archive lue par plages d'octets) ; MBTiles (SQLite). |

## 6. Projets open source

| Projet | Licence (vérifiée via les API GitHub/Codeberg) | Intérêt |
|---|---|---|
| OsmAnd | GPLv3 pour le code ; artwork en CC-BY-NC-ND 4.0, en partie propriétaire | Style « winter+ski », pistes OpenSnowMap déjà intégrées à ses cartes |
| Organic Maps | Apache-2.0 | Moteur hors-ligne |
| CoMaps (fork d'Organic Maps, Codeberg) | Apache-2.0 | Idem, actif |
| OpenTracks | Apache-2.0, déménagé sur Codeberg (le dépôt GitHub est archivé) | Enregistrement GPS sur téléphone |
| Trail Sense | MIT, actif | Outils hors-ligne |
| Gadgetbridge (Codeberg) | AGPL-3.0 | Montres sans cloud (pas Wear OS) |
| StreetComplete | GPL-3.0 | Contribution OSM |
| android/health-samples | Apache-2.0 | Exemple `ExerciseSampleCompose` pour Health Services |

## Pile technique recommandée

**Téléphone (Kotlin)**
- **Carte :** MapLibre Native Android, gratuit (BSD).
- **Fond vectoriel :** Plan IGN v2 (TMS `PLAN.IGN` et style `standard.json`), gratuit en Etalab 2.0, avec attribution IGN. À compléter par un PMTiles Protomaps de chaque zone, embarqué ou téléchargé (ODbL, gratuit).
- **Pistes :** extraites de `planet_pistes.osm.gz` d'OpenSnowMap (ODbL, gratuit), transformées côté serveur dans un outil compilé en GeoJSON ou PMTiles. Pour le Jura, s'y ajoutent les GPX de l'API Geotrek ENJ, une fois la licence confirmée par ENJ.
- **SCAN 25 :** seulement en option payante. Streaming seul gratuit sous 10 M transactions par an, mais avec une licence IGN à souscrire. Hors-ligne : **6 € HT par utilisateur et par an**.
- **Météo et avalanche :** API Météo-France (compte gratuit) pour le BERA du massif alpin et les observations, Open-Meteo pour la hauteur de neige modélisée. Open-Meteo est gratuit en usage non commercial ; l'offre commerciale est à chiffrer, prix **non vérifié**.
- **État des pistes :** pas de source ouverte. Il faut une convention avec Nordic France ou les gestionnaires (ENJ, mairie de Cervières), ou Apidae Connect (tarif inconnu). En attendant, on peut afficher seulement un lien vers les bulletins.

**Montre**
- App Wear OS séparée : Health Services `CROSS_COUNTRY_SKIING` (support à vérifier avec `getCapabilities`, repli sur `SKIING`) pour l'enregistrement GPS. Export vers Health Connect en `EXERCISE_TYPE_SKIING` avec `ExerciseRoute`.
- Carte sur la montre : petit PMTiles local avec MapLibre (à prototyper) ou Mapsforge. Google Maps est exclu à cause des calques non supportés.

**Coût de démarrage :** proche de 0 € (plus 25 $ une fois pour le compte Google Play, **non vérifié ici**). Les coûts variables possibles sont la licence SCAN 25 hors-ligne (6 € HT par utilisateur et par an), Apidae Connect et l'offre commerciale Open-Meteo.

**Non vérifié :**
- quotas du portail Météo-France ;
- prix d'Open-Meteo et d'Apidae ;
- MapLibre sur Wear OS ;
- autonomie GPS officielle de la Pixel Watch 2 ;
- licence des données ENJ ;
- rattachement BERA de Cervières ;
- page Nordic France de Cervières (timeout) ;
- détail OSM de Chapelle-des-Bois (Overpass saturé).

Fichiers de travail (sondes) : `research/raw/` (`wmts.xml`, `lic/bareme.pdf`, `lic/lic.txt`, `live/enj.html`, `live/nf.html`, `dt/tour.csv`, `dt/pac.csv`, `mf/bra.txt`, `feed.json`).
