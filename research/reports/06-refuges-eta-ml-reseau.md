# Rapport : refuges, ETA, ML/données, couverture mobile (Cervières et Chapelle-des-Bois)

Les bbox de Chapelle-des-Bois et de Cervières débordent sur la Suisse et sur l'Italie. Quand je l'indique, j'ai séparé ce qui est en France grâce aux contours des communes (geo.api.gouv.fr, départements 05, 25 et 39).

---

## 1. Refuges, abris et ravitaillement en hiver

### refuges.info (API sondée)
- **Endpoints** (doc https://www.refuges.info/api/doc/) : `GET /api/bbox`, `/api/massif`, `/api/point`, `/api/commentaires?id_point=`, `/api/contributions`, `/api/polygones`.
  - Formats : geojson, kml, kmz, gml, gpx, csv, xml, rss.
  - `detail=complet|fiche` ; `depuis=` permet des mises à jour incrémentales ; `nb_points` vaut 250 par défaut.
  - « aucune autentification ou clé d'API n'est requise ».
- **Licence** : « Les données du site Refuges.info sous sont licence CC By-Sa 2.0 ». Les données OSM renvoyées par la recherche sont sous ODbL. Le champ `copyright` du GeoJSON reçu dit la même chose.
- **Champs utiles en hiver** (relevés dans les réponses) :
  - `type` : cabane non gardée, refuge gardé, gîte d'étape, point d'eau, bâtiment à investiguer, passage délicat.
  - `etat` : « Fermé au public », « Inutilisable », « Détruite ».
  - `places`, `info_comp.{poele, cheminee, bois, eau, couvertures, latrines, manque_un_mur, places_matelas}`, `date.derniere_modif`, puis les commentaires datés avec photos.
  - Attention, `bois` signifie « Forêt à moins de 100m », pas un stock de bois.
  - **Il n'existe aucun champ « ouvert en hiver » ni « local d'hiver »**. Les fiches « refuge gardé » n'ont que eau, latrines et places. Le forum discute depuis 2023 et 2025 de faire une fiche séparée pour le local d'hiver, sans décision : https://www.refuges.info/forum/viewtopic.php?p=39592

| bbox | Total | Détail |
|---|---|---|
| Cervières | 22 | 8 cabanes non gardées (7 en FR), 1 refuge gardé, 1 gîte (Yourte des Ayes, « Fermé au public »), 10 points d'eau, 2 bâtiments « Inutilisable » |
| Chapelle-des-Bois | 69 | 57 cabanes non gardées, **dont 42 hors France** (côté suisse, presque toutes avec poêle = Oui), 15 en FR ; 6 gîtes d'étape (dont « Chez L'Aimé » fermé au public) ; 5 points d'eau ; 1 passage délicat |

Deux exemples :
- **id 648, Gîte de Vers le Col** (2140 m, 16 places, poêle Oui). Commentaire du 2025-11-13 : « présence d'un poêle efficace en tirage mais pas pour chauffer la pièce… robinet coupé ».
- **id 1428, Refuge des Fonts de Cervières** : la fiche ne contient aucune information sur l'hiver.

### OSM (Overpass), licence ODbL

| Tag | Cervières (FR / hors FR) | Chapelle-des-Bois (FR / hors FR) |
|---|---|---|
| tourism=alpine_hut | 2 / 3 | 7 / 0 |
| tourism=wilderness_hut | 2 / 1 | 4 / 2 |
| amenity=shelter | 10 / 0 (dont 5 abribus `public_transport`) | 19 / 40 (38 `weather_shelter` au total) |
| amenity=restaurant | 10 / 1 | 9 / 0 |
| opening_hours / winter_room / fireplace / ref:refuges.info | 2 / 0 / 1 / 5 | 7 / 0 / 42 / 53 |

- Taginfo, monde entier : `winter_room` n'est utilisé que 341 fois (yes 240, no 71). `ref:refuges.info` l'est 4749 fois, ce qui permet de joindre les deux bases.
- Le wiki définit winter_room comme « Small room to provide shelter outside the opening period of an alpine hut » (https://wiki.openstreetmap.org/wiki/Key:winter_room).

### Autres sources

| Source | Résultat |
|---|---|
| **Camptocamp** API (`api.camptocamp.org/waypoints?wtyp=hut\|shelter\|gite&bbox=` en EPSG:3857) | Licence par document : `"CC-BY-SA-3.0"`. Champs `custodianship`, `capacity` (non gardé), `capacity_staffed`, `heating_unstaffed`, `blanket_unstaffed`, `gas_unstaffed`, `matress_unstaffed`, `access_period`. Fonts de Cervières : `always_accessible`, 9 places non gardées, 62 gardées, période « 01/03 au 01/05 et du 15/06 au 15/09 ». Comptes : Cervières hut 3, shelter 1, gite 1 ; Chapelle-des-Bois hut 1, shelter 20 (refuges du Risoud), gite 2. |
| **FFCAM** (https://www.ffcam.fr/periode-hors-gardiennage.html) | Hors gardiennage, « une partie accessible et ouverte afin de remplir leur rôle d'intérêt général d'abri et de secours ». Ni liste des capacités hivernales ni API : il faut aller sur le site de chaque refuge. |
| **ENJ Geotrek** `admin.snowmap.espacenordiquejurassien.com/api/v2/` | La racine renvoie 403. `poi/` ne contient que **3 POI** en tout (types : point de vue, prestataires, salle hors-sac). `touristiccontent/` : 192 entrées, catégories Loisirs (132), Organismes (36), Commerces (24), **aucun restaurant ni hébergement**. 22 sont dans la bbox (ESF, loueurs, sites nordiques). `trek/` : 641 (149 dans la bbox) ; `sensitivearea/` : 25. **Ce n'est pas une source de lieux de ravitaillement.** |
| **DATAtourisme** | Licence Ouverte 2.0 (https://www.data.gouv.fr/datasets/datatourisme-la-plateforme-nationale-des-donnees-touristiques). CSV régionaux mis à jour chaque jour, export NT de 1,9 Go. L'API (https://api.datatourisme.fr/v1/docs) demande une clé, est limitée à 1000 req/h, `page_size` ≤ 100, avec filtres `geo_bounding` et `geo_distance`. Mes comptes : bbox Chapelle-des-Bois 430 POI dont 18 restaurants (source Décibelles Data), bbox Cervières 63 POI dont 10 restaurants (Apidae), par exemple Restaurant l'Arpelin (mis à jour le 2026-11-06), Les Clochettes du Risoux (2026-11-12). **La colonne `Periodes_regroupees` est vide pour les 28 restaurants** : les horaires d'hiver ne figurent pas dans le CSV. |

**Ce que ça permet dans l'app, et le coût.** On peut constituer un pack hors ligne par site : abris avec poêle, places et état, plus le dernier commentaire daté. On le rafraîchit chaque semaine avec `depuis=`, on fusionne avec OSM via `ref:refuges.info` et on prend la capacité du local d'hiver chez Camptocamp. Les lieux pour manger viennent de DATAtourisme et d'OSM, avec la mention « horaires non garantis, appeler ». Aucune donnée n'est payante. La contrainte est juridique : refuges.info et Camptocamp sont en CC BY-SA et OSM en ODbL, donc attribution obligatoire et partage à l'identique de la base dérivée si on la redistribue. Côté infrastructure, il suffit d'un job de build sur un serveur ou sur le poste du développeur, sans coût par utilisateur. La clé et le quota DATAtourisme imposent de précalculer côté build.

---

## 2. Estimation des temps (ETA)

| Méthode | Formule | Source |
|---|---|---|
| DIN 33466 | 300 m/h en montée, 500 m/h en descente, 4 km/h ; on ajoute la moitié du plus petit des deux temps au plus grand | https://de.wikipedia.org/wiki/Marschzeitberechnung (cite le Wegehandbuch DAV/ÖAV) |
| SAC (été comme hiver) | 4 km/h et 400 m/h ; descente à ski ≈ 1/3 du temps de montée (≈1200 m/h) | même page (cite SAC Bergsport Winter) |
| Munter | T = (km + Δh/100) / taux ; taux 4 en montée, 10 en descente à ski ; « adjust it based on testing » | https://backcountryaccess.com/de-de/blog/p/how-to-calculate-backcountry-touring-time-based-distance-elevation-gain |
| Tobler | W = 6·e^(−3,5·\|S+0,05\|) km/h ; ×3/5 hors sentier | https://en.wikipedia.org/wiki/Tobler%27s_hiking_function |
| Schweizer Wanderwege | Polynôme de degré 15 de la pente depuis 2006 (coefficients non trouvés). Leistungskilometer = (horizontale m + 10×montée + 2,5×descente)/1000 | Wikipedia DE ci-dessus ; PDF Schweizer Wanderwege 2008 (https://www.swisshiking.ch/de/media/download/f50a29d091281927c016f9c0bd155d7a528fb50c) |
| Skitourenguru (champ `time` de chaque itinéraire) | Ma régression sur 14 178 itinéraires : t ≈ 0,288 h/km + 0,208 h/100 m D+ (RMSE 0,30 h). Munter avec taux 4 : biais −0,26 h, RMSE 0,42 h. **Leur méthode n'est pas documentée.** | JSON public `https://www.skitourenguru.com/calc_data2/json/Alps_Symbols_900913.json` |

**Ski de fond, littérature scientifique :**
- **Bolger et al. 2015** (IJSPP, PMID 25671845, résumé lu via NCBI) : le temps de course se répartit en ~55 % de montée, 15-20 % de plat et 25-30 % de descente. Le skating est plus rapide que le classique de 9 % (hommes) à 11 % (femmes) au total, de 12 à 15 % en montée, de 8 à 13 % sur le plat et de 2 et 1 % en descente.
- **Moxnes et al. 2013** (PMID 24379718) : modèle de bilan de puissance ; +10 % de friction ou de traînée donne +3-4 % de temps.
- **Carlsson et al. 2011** (PMID 21607888) : simulation numérique (gravité, friction, traînée, puissance).
- **Almqvist 2022** (PMID 35392593) et **Colbeck 1994** (PMID 8064975) : la friction dépend de la température et de l'humidité de l'air et de la neige, et du type de neige ; « the important processes operate… at the ski base temperature ».
- **Neige non tracée** : Richmond 2019 (PMID 30487109) donne un coefficient de terrain η pour l'équation de Pandolf, avec z = h·(1−ρ/1,186) et η = 0,0005z³+0,0001z²+0,1072z+1,2604. C'est pour la marche ; **je n'ai trouvé aucun modèle publié pour du ski hors piste à faire la trace.**

**Applications existantes :**
- **komoot** : niveau de forme auto-déclaré ; « Komoot's current fitness settings don't rely on scientific measures of physical fitness » ; temps en mouvement seulement (https://www.komoot.com/adventure-hub/186OZJztsNtyEOgjSnbQkf/…).
- **Strava** : « Your estimated moving time for a route is calculated by your 4-week average speed or pace », ajusté selon le profil de la personne qui consulte (https://support.strava.com/en-us/articles/15401971-…).
- **Outdooractive** : non vérifié.

**ETA personnalisée, prior art :**
- Wood et al. 2023 (arXiv 2303.16065, CC BY) : modèle GLM qui ajoute la pente du terrain, le type de terrain et les obstacles, sur 88 000 km de traces ; meilleur que les formules classiques.
- Code ouvert : `ae137/HikingTimePredictor` (« 40-50 GPX tracks » suffisent, licence NOASSERTION) et `tarutaru2048/HikingTTE` (aucune licence). Aucun des deux n'est réutilisable sans autorisation.
- Campbell et al. 2019 (DOI 10.1016/j.apgeog.2019.03.008) : seule la référence bibliographique est vérifiée, je n'ai pas pu lire le texte.

**Ce que ça permet dans l'app, et le coût.**
- **Prior physique** : P = v·m·g·(sinα + μ·cosα) + ½ρ·CdA·v³. On prend μ par classe de neige et de température, puis un facteur de technique (classique ou skating) tiré de Bolger.
- **Calibration bayésienne par utilisateur** : on ajuste peu de paramètres (puissance soutenable, multiplicateur de μ, vitesse plafond en descente) sur les traces que **l'app enregistre elle-même**.
- C'est ma proposition, pas une méthode publiée. Le calcul tient sur l'appareil en Kotlin. Coût de données nul.

---

## 3. ML et données

### a) Skitourenguru
- **Méthodes** : QRM, GRM et **SLABS**, un GAM binomial ajusté sur « 57.8 thousand km of GPS-tracked ski tours and 1,250 accidents recorded in Switzerland » (ISSW 2024 P12.2, https://arc.lib.montana.edu/snow-science/objects/ISSW2024_P12.2.pdf). Le routage se fait par Dijkstra sur une surface de coût avec GRASS `r.walk`, ouvert à tout l'arc alpin depuis décembre 2024 (ISSW 2024 O11.9).
- **Données** : le MNT français est l'IGN LiDAR HD à 5 m, recalculé avec un filtre SMRF ; carte ATHM à 10 m sur les Alpes.
- **Couverture** : le bundle JS contient « © Meteo France » et renvoie vers le BERA. Le fichier `RiskSummary.json` montre une collection France de 3 725 itinéraires. J'en compte **157 dans la bbox de Cervières** (par exemple Cervières → Le Chenaillet) et **0 dans le Jura et à Chapelle-des-Bois**.
- **Licence** : aucune licence ouverte. Le site se dit « free of charge and without advertising » et décline toute garantie. Il faudrait un accord pour réutiliser les données.

### b) auto-ATES
- `AutoATES/AutoATES-v2.0` est sous GPL-3.0. Il prend en entrée un MNT et une densité forestière entre 5 et 30 m. Il a été testé en Norvège, au Canada et aux États-Unis. Le calcul de toute la Norvège a pris 30 jours sur AWS pour environ 1 600 USD (NHESS 24-1779-2024).
- La v3.0 a fait l'objet d'un atelier ISSW 2026 (dépôt MIT) avec ALOS 30 m, forêt Sentinel-2 et com4FlowPy.
- **Je n'ai trouvé aucune couche ATES publique pour la France.** La seule couche automatique de terrain sur les Alpes françaises est l'ATHM de Skitourenguru, qui est propriétaire.

### c) Couverture neigeuse satellite

| Produit | Résolution | Accès et licence | Constat |
|---|---|---|---|
| CLMS FSC (Sentinel-2) | 20 m, au rythme des passages (5 j), NRT | Règlement UE 1159/2013 : libre avec mention de la source ; WEkEO `EO:CRYO:DAT:HRSI:FSC` | La collection STAC de CDSE existe mais **ne renvoie aucun item**, ni sur les deux bbox (25 oct. au 15 nov. 2026) ni au niveau de la collection. Le CLMS indique que le chargement « should finish by November 2026 ». L'API resto de cryo.land renvoie 503. |
| CLMS GFSC (Sentinel-1 et 2) | 60 m, quotidien sans trous | idem | Méthode refondue en 2025 |
| CLMS WDS / SWS | 60 m | idem | Neige humide ou sèche |
| Theia Neige (Let-it-Snow) | 20 m | WMS et WMTS ; licence non vérifiée | Couverture France : « Alpes, Pyrenées, Massif Central, Vosges, Corse ». **Pas le Jura.** « pas fiable dans les zones de forêts denses » ; les pentes raides à l'ombre peuvent sortir « pas de neige ». |
| MODIS MOD10A1 / MYD10A1 | 500 m, quotidien | dans le STAC de CDSE | Trop grossier pour dire si on peut sortir des pistes |

Pour répondre à « assez de neige hors piste ici aujourd'hui ? », le satellite donne la présence de neige, pas sa hauteur. En forêt, et le Risoud est une forêt, il ne sert pas.

### d) Inférer le damage à partir de traces GPS
- **Nordic Pulse** utilise le GPS des dameuses (« syncs the data with our server every few seconds », enregistrement hors ligne antidaté). Leur liste compte 21 sites en France, dont **Domaine nordique d'Arvieux**, Les Rousses, Monts Jura-La Vattay et Haut-Saugeais. **Ni Cervières ni Chapelle-des-Bois.**
- Je n'ai trouvé aucune publication qui déduise le damage des traces de skieurs.

### e) Hauteur de neige
- **Météo-France, « Données de Modèle de Simulation Nivologique »** (Licence Ouverte, data.gouv), CSV quotidien par massif × altitude (pas de 300 m) × exposition (8 orientations et plat) × pente (0, 20 ou 40°). Variables : `neigetot` (cm), `equivalent_eau` (mm), `ecoulement`.
  - 59 massifs, dont 13 Thabor, 16 Pelvoux, 17 Queyras, **55 Haut-Doubs et 56 Haut-Jura**.
  - Dernière date disponible le 14/11 : 2026-11-13, soit J-1.
  - Au 13/11, sur le plat : Queyras à 0 cm jusqu'à 2400 m puis 34 cm à 2700 m ; Haut-Jura à 0 cm partout.
- **Stations horaires BDClim** (Licence Ouverte) avec la colonne `NEIGETOT` :
  - Alpes : **CERVIERES 1614 m** (05027001, 16 347 valeurs), ARVIEUX 1825 m, VILLAR ST PANCRACE.
  - Jura : MOUTHE 940 m (seulement 352 valeurs), LA BOISSAUDE ROCHEJEAN 1222 m, LA PESSE 1133 m, PONTARLIER.
  - Le fichier en masse a environ 1,5 jour de retard (dernière observation 2026111304).

### f) Strava (point ajouté par le coordinateur)
Sources : API Agreement et API Policy, toutes deux « Effective Date: June 1, 2026 » (https://www.strava.com/legal/api, https://www.strava.com/legal/api_policy).

- **(a) Envoi d'activités : oui.** Agreement 7.1 : « Your Developer Applications may include the option to upload activities ». Le scope `activity:write` donne « access to create manual activities and uploads ». Formats FIT, TCX, GPX. `NordicSki` et `BackcountrySki` figurent dans la référence API.
- **(b) Lecture de l'historique : techniquement oui** (`activity:read_all`), **mais :**
  - Policy 6.2 : « You may not retain Strava Data in your cache for longer than seven (7) days ».
  - Policy 5.5 interdit de stocker des « data derived from Strava Data » dans un « Persistent Index ».
  - Policy 5.4 : « You may not process… Strava Data… for the purposes of analytics, analyses… ».
  - Une calibration ETA persistante à partir de l'API est donc juridiquement très risquée.
  - **Alternative propre** : Policy 6.6, l'utilisateur garde son « Bulk Data Export Tool ». L'utilisateur importe lui-même ses fichiers ; c'est mon interprétation, non vérifiée juridiquement.
- **(c) Novembre 2024, puis aujourd'hui :**
  - L'annonce officielle du 15/11/2024 (https://communityhub.strava.com/developers-api-7/api-agreement-update-how-data-appears-on-3rd-party-apps-7636) : « explicitly prohibit third parties from using any data obtained via Strava's API in artificial intelligence models ».
  - Texte actuel, Policy 2.3 : « may be displayed or disclosed… only to that user ».
  - Policy 5.3 : « You may not use the Strava API Materials or Strava Data, directly or indirectly, in connection with the development, training, evaluation, or operation of any AI Application ». Cela couvre aussi les données « aggregated… anonymized ».
  - « AI Application » n'est défini dans aucune des deux pages, donc une simple régression pourrait tomber sous le coup de l'interdiction.
- **(d) Quotas** (https://developers.strava.com/docs/rate-limits/) :
  - Par défaut : 200 req/15 min et 2000/jour au total ; 100 req/15 min et 1000/jour pour les endpoints hors upload.
  - Une nouvelle app a une capacité de 1 athlète, puis 10 (400/15 min, 4000/jour). Au-delà, il faut une revue.
  - « A Strava subscription is a prerequisite for creating an app. »
  - Policy 3.3 : les apps Standard Tier sont soumises à des « subscription requirements… the developer or specified end users ».
  - Policy 5.2 interdit tout usage « competitive to Strava ».

**Ce que ça permet dans l'app, et le coût.**
- **Gratuit, licence ouverte** : hauteur modélisée par massif, altitude et exposition (y compris le Jura), plus la station de Cervières, pour un indicateur « neige probable ». Un traitement serveur par jour qui pèse quelques Mo.
- **Payant en travail ou en accord** : Skitourenguru (accord nécessaire), auto-ATES France (à calculer soi-même, GPL), FSC (accès CDSE pas encore opérationnel).
- **Strava** : à limiter à l'envoi en écriture seule. Pour l'historique, passer par l'import des fichiers de l'export Strava.

---

## 4. Couverture mobile

- **ARCEP « Mon Réseau Mobile »** (https://www.data.gouv.fr/datasets/mon-reseau-mobile, Licence Ouverte) :
  - Doc : https://data.arcep.fr/mobile/couvertures_theoriques/documentation_couverture.md
  - Format : GeoPackage compressé en 7z, polygones en Lambert-93 (EPSG 2154), un fichier par opérateur, techno et usage pour toute la métropole (107 à 349 Mo compressés, 0,7 à 1 Go une fois extraits).
  - Niveaux TBC, BC et CL (« à l'extérieur des bâtiments dans la plupart des cas »).
  - Mise à jour trimestrielle pour la 4G, semestrielle pour la 2G et la 3G.
  - Ce sont des simulations (« relief, fréquences…, sursol »). **La résolution n'est pas documentée.** Pas de carte 5G ; la carte 2G de Free n'est plus publiée depuis le T4 2024.
- **Ce que j'ai calculé** (2G/3G voix, T2 2026, partie française des bbox) :

| | Bouygues | SFR | Free | Orange | Au moins un opérateur | **Aucun réseau voix** |
|---|---|---|---|---|---|---|
| Cervières (225,6 km²) | 69,4 % | 69,3 % | 54,2 % | 37,7 % | 77,3 % | **51,2 km² (22,7 %)** |
| Chapelle-des-Bois (172,7 km²) | 77,7 % | 84,8 % | 76,9 % | 73,2 % | 89,8 % | **17,6 km² (10,2 %)** |

- **Pourquoi l'union de tous les opérateurs compte** : réponse ministérielle publiée au JO du 20/01/2026 (QE n° 4594, https://www.assemblee-nationale.fr/dyn/17/questions/QANR5L17QE4594).
  - Le 112 « a la particularité de pouvoir être borné par toutes les antennes » (c'est le texte de la question).
  - La réponse ajoute que depuis juin 2024, « les numéros 15, 17 et 18 peuvent également être appelés en 4G en itinérance nationale sur les réseaux des quatre opérateurs, en plus du 112 ».
- **AML** : le rapport EENA 2023 indique pour la France une activation en 2020 sur Android et iOS, avec les numéros 112, 15 et 18. Je n'ai pas pu lire l'état des cases à cocher dans le PDF extrait. La page Google ELS (https://www.android.com/safety/emergency-help/emergency-location-service/how-it-works/) ne nomme pas la France (« most of Europe »). Le SMS de données exige une SIM ; la version HTTPS fonctionne en Wi-Fi ; « Google does not get any emergency locations ».
- **SOS par satellite sur Pixel** (https://support.google.com/pixelphone/answer/15254448?hl=fr) : « disponible sur tous les appareils Pixel depuis le Pixel 9, à l'exception du Pixel 9a ». La France est dans la liste. **Les Pixel 8 et 8 Pro ne sont pas pris en charge.**

**Ce que ça permet dans l'app, et le coût.** On peut livrer, dans chaque pack de site, un polygone « aucun réseau, même pour le 112 » découpé sur la zone. L'app alerte avant d'y entrer (« prévenez quelqu'un de votre heure de retour maintenant »). Le calcul demande environ 1 Go de téléchargement par trimestre, sur le poste du développeur ou un serveur. Le pack final pèse quelques centaines de Ko et ne coûte rien par utilisateur.

---

## Non vérifié
- Les horaires d'hiver des restaurants dans l'export NT ou l'API DATAtourisme : j'ai seulement constaté qu'ils sont absents du CSV.
- La latence chiffrée de FSC : 3 h ou 12 h selon des sources secondaires, la FAQ CLMS ne donne pas de chiffre.
- La licence des produits Theia.
- Le nom exact du modèle nivologique publié (SAFRAN-Crocus), absent des documents que j'ai ouverts.
- L'unité de `NEIGETOT` dans les fichiers de stations.
- Le massif SAFRAN de Chapelle-des-Bois : 55 ou 56.
- La méthode de calcul du `time` de Skitourenguru et son éventuelle personnalisation.
- La formule d'Outdooractive, le contenu de Campbell 2019, le modèle GAP de Strava (lien en 403).
- Les coefficients du polynôme suisse.
- Les cases cochées du rapport EENA pour la France.
- La définition de « AI Application » chez Strava, et l'exigence d'abonnement pour les utilisateurs finaux.
- Que le 112 fonctionne sur la Pixel Watch 2 Wi-Fi seule : c'est déduit de l'absence de radio cellulaire, pas lu sur une page Google.
- La localisation exacte des 42 cabanes hors France : je les suppose côté suisse du Risoud, sans l'avoir confirmé.

Les fichiers de travail sont dans `research/raw/`. Les scripts sont dans `research/raw/scripts/cov_fr.py` et `research/raw/scripts/dt.py`. J'ai supprimé les 3,2 Go de GeoPackages ARCEP extraits. Les archives .7z (983 Mo) restent dans `research/raw/arcep/dl`.
