# Applications et sources pour le ski de randonnée nordique à Cervières et Chapelle-des-Bois

**En bref :**
- Aucune grande base de topos n'a de catégorie pour le ski de randonnée nordique (SRN). La seule base qui lui est consacrée est skirandonneenordique.com.
- Les cartes OSM ne contiennent quasiment rien d'utile pour le SRN.
- Cervières dépend du BERA **Queyras**. Le Jura n'a pas de BERA.
- Dans le Haut-Jura, la contrainte forte vient des arrêtés de protection de biotope (APPB) sur les forêts d'altitude. Leurs zones sont publiées par l'API Biodiv'sports, à condition de passer le paramètre `period`.
- Sur la Pixel Watch 2, deux applis seulement sont attestées sans téléphone avec carte hors ligne : WristNav (seule à avoir un « Back to Start » attesté) et le nouveau komoot.

---

## 1. Sources de topos et d'itinéraires SRN

### 1a. Bases de topos

| Source | Catégorie SRN ? | Cervières | Chapelle-des-Bois | Accès aux données |
|---|---|---|---|---|
| **Camptocamp** | **Non.** La liste des activités en compte 11, sans nordique ([attributes.py l.4-16](https://github.com/c2corg/v6_api/blob/master/c2corg_api/models/common/attributes.py)). Le SRN est saisi en `skitouring`. | Voir détail ci-dessous | Voir détail ci-dessous | API ouverte sans clé, licence CC-BY-SA-3.0 (champ `license`) |
| **Skitour.fr** | **Non.** Les seuls filtres sont l'exposition, l'orientation et le massif. | Massif « Queyras - Alpes Cozie N » (id 6) : contient le topo 8984 « Col de l'Izoard, par la piste de ski de fond » | Massifs « Jura » (id 37, 44 topos sur la page) et « Jura Vaudois » (id 257). Topos utiles : 6399 « Roche Champion, en boucle depuis la Chapelle-des-Bois », 7109 « Mont d'or, par la GTJ… », 3234 et 3755 (Mont d'Or) | API avec une clé par application, limitée à 1000 requêtes par 24 h. Topos sous CC BY-SA 4.0 ([skitour.fr/api](https://skitour.fr/api/)). Je ne l'ai pas appelée faute de clé. |
| **skirandonneenordique.com** | **Oui, c'est la seule base SRN dédiée trouvée.** 108 topos, filtrables par zone. | 13 topos Briançonnais et 3 Queyras. Voir détail ci-dessous. | Jura 2, Jura Sud 8, Jura Nord 1, Suisse > Jura 1. Voir détail ci-dessous. | Pas d'API. Licence non indiquée. |

**Camptocamp, comptages par l'API :**
- **Zone Cervières** (bbox 44.77,6.66–44.92,6.86, qui inclut le versant italien) :
  - 184 itinéraires au total, dont 136 en skitouring, 9 en raquettes et 26 en randonnée.
  - En réunissant ski et raquettes, on obtient 137 itinéraires : 112 en France, 20 en Italie, 5 frontaliers.
  - 21 ont une cotation ski de 2.1 ou moins.
  - Exemples faciles : 971502 Col de l'Izoard versant S (cotation 1.3), 159296 Col des Ourdeis depuis Le Laus (1.3), 523459 Col de Bousson en boucle par la Cerveyrette (raquettes R1), 1510627 Haute route des Escartons.
- **Zone Chapelle** (bbox 46.54,6.02–46.68,6.20) :
  - 12 itinéraires au total, dont 5 en skitouring et 3 en raquettes.
  - Exemples : 867027 et 895760 (étapes 5 et 6 de la GTJ en ski de fond, Mouthe → Chapelle → Les Rousses, rédigées en catalan), 961557 La Citadelle, 1331141 Roche Champion (1.1).
- **Recherche plein texte :** « ski nordique » ne donne aucun résultat. « randonnee nordique » en donne un seul, en Lozère.

**skirandonneenordique.com, topos utiles :**
- Côté Cervières :
  - [Cervières – Capanna Mautino](https://www.skirandonneenordique.com/topos/cervieres-capanna-mautino/) : 19,6 km, +728 m, difficulté moyenne, GPX.
  - [Souliers – Arvieux – Cervières](https://www.skirandonneenordique.com/topos/souliers-arvieux-cervieres/) : 20 km, +950/−1050 m, GPX. La page impose DVA, pelle et sonde pour l'Izoard.
  - Haute Route des Escartons.
- Côté Jura :
  - [Traversée Mouthe – Col de la Faucille](https://www.skirandonneenordique.com/topos/traversee-mouthe-col-de-la-faucille/) : 60 km en 4 jours, par le Risoux, la Grande Verrière et Bois d'Amont. Pas de GPX mentionné.
  - [Super-Longevilles – Mont d'Or](https://www.skirandonneenordique.com/topos/super-longuevilles-mont-dor-la-coquille/) : 14,5 km, +426 m, GPX.

### 1b. Itinéraires officiels

- **GTJ à ski de randonnée nordique.**
  - Tracé : Mouthe → Giron, 100 km en 8 jours ([jura-tourism](https://www.jura-tourism.com/itineraire/la-grande-traversee-du-jura-a-ski-de-rando-gtj-a-ski-randonnee-nordique/)).
  - Balisage : « plaquettes orange » et « fanions violets ». Le parcours se fait « hors de toutes traces ».
  - Ouverture : de Noël jusqu'à environ le 25 mars. GPX téléchargeable.
  - Incohérence : la page [montagnes-du-jura](https://en.montagnes-du-jura.fr/sit/la-grande-traversee-du-jura-a-ski-de-randonnee-nordique-gtj-a-ski-de-randonnee-nordique) annonce 130 km dans le texte et 100 km dans l'encadré.
  - gtj.asso.fr répond 403 : non consulté.
- **Site nordique ENJ de Chapelle-des-Bois** ([fiche ENJ](https://www.espacenordiquejurassien.com/annuaire/site_nordique_de_chapelle_des_bois~f09d385f987b8cede6ba90cb5af.html)) : « 14 pistes de ski de fond (127 km) », « 7 pistes de raquettes (62 km) », « GTJ ski de rando : Chapelle des Bois – Bellefontaine : 9km ».
- **Les Rousses** : trois itinéraires SRN courts (Balancier 1 km, etc.), « Accessibles avec le pass Jura sur Léman » ([jura-tourism](https://www.jura-tourism.com/vivre-le-jura/pratiquer/ski-sports-hiver/ski-de-randonnee-nordique-dans-le-jura/)).
- **Cervières** ([cervieres.fr](https://cervieres.fr/domaine-nordique-2/domaine-nordique/)) :
  - « Depuis la plaine du Bourget rejoignez l'Italie par un itinéraire balisé ! (non damé) ».
  - Le domaine compte 53 km de pistes et 46 km d'itinéraires piétons et raquettes.

### 1c. Autres sources

- **Nordic France :** pas de page « ski nordique de randonnée » trouvée.
- **Guides papier :** aucun topo-guide spécifiquement SRN trouvé pour le Queyras, la Cerveyrette ou le Haut-Jura. Le Toponeige Queyras (Volopress) traite du ski de rando alpin, et je ne l'ai vu que dans des résultats de recherche.
- **Clubs :** le CAF Île-de-France organise des sorties SRN dans le Haut-Doubs, mais je ne l'ai vu que par la recherche.
- **Wikiloc :** répond 403.
- **Visorando :** sa [page premium](https://visorando.com/premium.html) ne liste aucune activité ski.

### 1d. OSM (Overpass)

Sémantique des tags d'après le [wiki OSM](https://wiki.openstreetmap.org/wiki/Key:piste:grooming) :
- `nordic` + `backcountry` = « Un-prepared itinerary… tracks are made manually by skiers ».
- `skitour` + `backcountry` = « Rough itinerary… no operator ».

| Tag | Cervières | Chapelle |
|---|---|---|
| `piste:type=skitour` | 2 voies, 13 km, **toutes deux en Italie** (Rhuilles–Monte Terra Nera, Claviere–Monte de la Plane). **0 côté français.** | 0 |
| `nordic` + `backcountry` (voies) | 0 | 5 voies, 2,5 km (Chemin du Sablon, Chemin des Mines, vers 46.62 N / 6.19 E) |
| `nordic` + `backcountry` (relations) | 1 : « Traversée du Queyras – Chanteloube », 9,3 km, entre 44.72 et 44.78 N, donc surtout au sud de la bbox | 1 : « Ravier », 8,1 km, mais ses voies sont taguées `classic;skating`, ce qui est incohérent |
| `downhill` + `backcountry` | 3 voies, 4,5 km (hors-piste de Montgenèvre/Claviere, sans intérêt pour le SRN) | 0 |
| `hike` + `backcountry` (raquettes) | 3 voies, 0,3 km | 21 voies, 11,1 km |
| Pistes nordiques damées (pour comparaison) | environ 25 km cartographiés, contre 53 km annoncés | environ 268 km |

## 2. Applications et données utiles spécifiquement pour le SRN

| Besoin | Constat vérifié | Source |
|---|---|---|
| **Pentes > 30°, gratuit** | La couche IGN « Carte des pentes » (`GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN`) est sur data.geopf.fr/wmts sans clé : une tuile z14 à Cervières a renvoyé un PNG de 63 KB (HTTP 200). Côté Suisse (Risoud), la couche swisstopo `ch.swisstopo.hangneigung-ueber_30` existe. | GetCapabilities, [geo.admin](https://api3.geo.admin.ch/rest/services/api/MapServer) |
| Pentes dans OsmAnd | **Payant :** « The Topography plugin is a paid feature », inclus dans Maps+ ou Pro | [doc OsmAnd](https://osmand.net/docs/user/plugins/topography/) |
| Pentes dans Visorando | Premium à 25 €/an : « visualiser l'inclinaison des pentes supérieures à 30° » | [visorando](https://visorando.com/premium.html) |
| Pentes dans Whympr | **Contradictoire.** Un test du 20 juin 2024 range les pentes et les zones nature dans le gratuit ; un article de 2025 les range dans le payant. | [outzer](https://www.outzer.fr/avis/app/whympr/whympr-1/0/10099.html) |
| **BERA à Cervières** | Massif **Queyras (code 17)**. Test point-dans-polygone sur le fichier `Metadata_massif_DP_2024` (copie GitHub d'un tiers) : le village, Le Laus, Les Fonts, l'Izoard et Rochebrune sont tous dans « Queyras ». | [geojson](https://raw.githubusercontent.com/BryanBradfo/avalanches-prediction/HEAD/data/Metadata_massif_DP_2024_20240215.geojson) |
| **Pas de BERA dans le Jura** | BERA « produits à l'échelle de 36 massifs pour les Alpes, les Pyrénées et la Corse ». Pour le Jura, Météo-France « diffuse des messages d'avertissement » (réponse publiée au JO le 11 juin 2024). Le fichier des massifs ne contient aucun massif Jura. | [AN QE 14701](https://www.assemblee-nationale.fr/dyn/16/questions/QANR5L16QE14701.pdf) |
| Ce qui remplace le BERA côté Suisse | Le SLF couvre « when there is sufficient snow cover, the Jura as well » | [SLF](https://slf.ch/en/avalanche-bulletin-and-snow-situation/about-the-avalanche-bulletin/the-avalanche-bulletin) |
| Skitourenguru | Le code de l'application contient une région `fr: France` créditée « © Meteo France ». La couverture du Queyras n'est pas vérifiée. Il évalue des itinéraires de ski de rando, pas de SRN. | bundle de skitourenguru.com |
| Routage hors-piste tenant compte de la neige | **Aucune application vérifiée ne le fait.** | — |
| Data-avalanche | Association, 6 671 avalanches recensées. Couverture et API non indiquées sur la page. | [data-avalanche.org](https://www.data-avalanche.org/) |
| Hauteur de neige | Open-Meteo `snow_depth` répond pour les deux sites. Attention : l'altitude du modèle était de 2619 m pour mon point près des Fonts, à vérifier contre l'altitude réelle (le point est peut-être mal placé). L'API Skitour a un endpoint `/api/conditions`. | appel API |
| Forêt (Jura) | Couches IGN publiques : `LANDCOVER.FORESTINVENTORY.V2` (BD Forêt V2), `IGNF_MASQUE-FORET.2021-2023`, `LANDCOVER.HR.TCD.CLC15` (couvert arboré à 20 m), `FORETS.PUBLIQUES`, et `PROTECTEDAREAS.PRSF` (points de rencontre des secours en forêt, utile pour la sécurité) | GetCapabilities |
| Suivi sans réseau | Le SOS par satellite est réservé aux Pixel 9 et suivants (« à l'exception du Pixel 9a ») : **le Pixel 8 n'y a pas droit**. La Pixel Watch 2 Wi-Fi n'a pas de LTE. | [Google](https://support.google.com/pixelphone/answer/15254448?hl=fr) |

## 3. Conditions pratiques sur les deux sites

### Cervières

**Tarifs 2026-2027** ([affiche des tarifs](https://cervieres.fr/wp-content/uploads/sites/268/2026/08/AFFICHE-TARIFS-REDEVANCES-2026-2027.pdf)) :

| Formule | Tarif |
|---|---|
| Journée ski de fond | 16 € adulte, 8 € jeune |
| 3 heures | 12 € |
| Contrôle sur piste sans redevance | 25 € |
| Multi-activité (piéton, raquette, rando) | 3 € la journée |
| Pass évasion | 5 € la journée |

**Règles d'accès :**
- Redevance : « LA REDEVANCE DE SKI DE FOND EST OBLIGATOIRE POUR L'ACCES AUX PISTES ».
- **Izoard :** « A compter de la saison 2026/2027, la piste du col d'Izoard deviendra un itinéraire en accès libre avec un damage très limité » ([cervieres.fr](https://cervieres.fr/domaine-nordique-2/domaine-nordique/)). L'office de tourisme recommande le trio DVA-pelle-sonde et d'éviter les pentes de plus de 30° ([hautesvallees](https://www.hautesvallees.com/lizoard/en/equipement/ski-touring-on-the-izoard-pass-cervieres/)).
- **Les Fonts :** « ouvert uniquement à partir des vacances de février ». **Pendant votre semaine de début janvier, la vallée des Fonts ne sera pas damée.**
- Accueil : le foyer est ouvert à partir du 19 décembre 2026.
- Hors pistes : les textes maintiennent « l'accès libre et gratuit au milieu naturel » ([fiche juridique CD73](https://www.savoie.fr/upload/docs/application/pdf/2022-04/cd73_fiches_juridiques-8_activites_nordiques_hd.pdf)).

**Zones Biodiv'sports en janvier** (bbox Cervières) : 8 zones.
- 6 « Aigle royal » et 1 « Grand-duc » : elles concernent les pratiques aériennes et verticales, pas le ski.
- 1 « Tétras lyre – hiver » (pratique terrestre) : 0,2 km², centrée vers 44.826 N / 6.755 E.

### Chapelle-des-Bois

**Redevance :** le règlement ENJ s'applique aux « pistes de ski de fond balisées et régulièrement entretenues » (article L2333-81 du CGCT) ([règlement ENJ 2023-24](https://www.espacenordiquejurassien.com/csx/scripts/downloader2.php?filename=T004/fichier/7b/d5/FR.m825n701c326&mime=application/pdf&originalname=ENJ_R_glement_de_Service_Redevances_Nordique_MASSIF-NATIONAL_SEUL_2023-2024.pdf)).
- Hors piste, aucun pass n'est dû.
- Je n'ai pas trouvé de règle écrite sur la simple traversée d'une piste damée.

**APPB Forêts d'altitude du Haut-Jura (département 39)** :
- Il couvre 5 massifs **dont le Risoux**, soit 4 334 ha sur 12 communes.
- Du 15 décembre au 30 juin, l'accès est limité aux itinéraires autorisés ([préfecture du Jura](https://www.jura.gouv.fr/Actions-de-l-Etat/Environnement/Nature-et-biodiversite/APPB-Forets-d-altitude-du-Haut-Jura)).
- L'ENJ écrit, à propos de ces zones : « En hiver, il est strictement interdit de sortir des itinéraires de ski de fond ou de raquettes matérialisés » ([ENJ environnement](https://www.espacenordiquejurassien.com/environnement.html)).
- La [carte interactive](https://carto.groupe-tetras-jura.org/) est hébergée sur MapTiler, sans téléchargement ouvert.

**Biodiv'sports, piège de l'API :** sans paramètre, l'API ne renvoie que les zones du mois en cours (novembre). Sur la bbox France, cela donne 236 zones par défaut, contre 1358 avec `period=ignore` et 794 avec `period=1`.

Avec `period=2,3`, la bbox Chapelle renvoie 6 zones, dont :
- **« Grand tétras – hiver »** (Parc du Haut-Jura) : 19 polygones pour 404 km² au total. 3 de ces polygones touchent la bbox. Le point 46.56 N / 6.10 E (Risoux) est dedans, le village n'y est pas.
- Hors bbox, vers Mouthe et Chaux-Neuve (46.70–46.79 N) : l'**« APPB des Forêts d'altitude du Haut-Doubs »** (zones cœur Risol nord et sud, Le Verdet), 12,4 km². Ses règles sont données en pictogrammes, illisibles ici.
- Il existe aussi une zone de quiétude volontaire aux Villedieu ([groupe-tetras](https://groupe-tetras-jura.org/zqvfs/)).

**Côté Suisse :** le district franc fédéral du Noirmont (Vaud) interdit toute l'année les sports d'hiver « en dehors de pistes et d'itinéraires balisés » (code R170). Je n'ai trouvé aucune zone de tranquillité dans la zone 5.95–6.45 E / 46.5–46.8 N.

**Applis qui affichent les zones Biodiv'sports** ([biodiv-sports.fr](https://biodiv-sports.fr/)) : Camptocamp, Visorando (avec connexion), Geotrek, Skitour, Whympr (filtre « Zones nature sensible »).

## 4. Pixel Watch 2

| Appli | Sans téléphone | Carte hors ligne sur la montre | Suivi d'un GPX / retour au départ | Source |
|---|---|---|---|---|
| **WristNav** | Oui | Oui | GPX oui, **« Back to Start »** oui. Trace et boussole non confirmées. Gratuit. | [site](https://boothy91.github.io/wrist-nav-site) |
| **komoot** (article du 21.10.2026) | Oui | Oui | Navigation et enregistrement oui, recalcul vers l'itinéraire. Retour au départ non mentionné. Pixel Watch listée. | [tour-magazin](https://www.tour-magazin.de/en/buying-advice/electronics/bike-computers-sat-navs-trackers/komoot-for-wear-os-new-smartwatch-app-provides-navigation-without-a-smartphone/) |
| Mapy.com | Enregistrement seulement | « The map is available online only » | Navigation uniquement avec le téléphone. Premium obligatoire. | [help.mapy](https://help.mapy.com/smart-watches/) |
| Locus | Non | — | — | [wiki OSM Wear OS](https://wiki.openstreetmap.org/wiki/Wear_OS) |
| Strava | Oui | Partielle (« only the current position ») | Non | wiki OSM |
| Outdooractive | Inconnu | — | — | wiki OSM |
| SityTrail | L'application Wear OS existe (même appli sur téléphone et montre). Le fonctionnement sans téléphone n'est pas confirmé. | — | — | [wiki SityTrail](https://www.sitytrail.com/en/wiki/0b18b9eb-dbb6-4cb3-b660-47570570916b/) |
| Google Maps | Cartes hors ligne synchronisées depuis le téléphone | Position seulement | « itinéraires… à pied ne sont pas disponibles hors connexion » | [Google](https://support.google.com/googlepixelwatch/answer/15395290?hl=fr) |

**Matériel de la Pixel Watch 2** ([the5krunner](https://the5krunner.com/specs/google-fitbit/pixel-watch-2/)) : boussole, magnétomètre, baromètre ; GPS, GLONASS, BeiDou, Galileo et QZSS. La bi-fréquence n'est pas indiquée.

---

## Non vérifié
- Mon test n'a pas montré que la couche gratuite des pentes IGN se charge réellement dans Locus, OsmAnd, Iphigénie, Cartes IGN ou WeTrek. Leurs offres pentes et leurs prix ne sont pas vérifiés non plus.
- Les apps Wear OS de WeTrek, Iphigénie, Visorando, Cartes IGN et Whympr, ainsi que celle d'OsmAnd (aucune trouvée).
- Le fonctionnement sans téléphone d'Outdooractive et de SityTrail. Les notes de version récentes de Mapy, qui contrediraient la page d'aide.
- Le contenu de gtj.asso.fr (403) et de Wikiloc (403).
- Le texte de l'APPB du Haut-Doubs (INPN répond 403) et la liste nominative des communes de l'APPB du Jura.
- Une règle écrite sur la traversée des pistes damées dans le Jura, et les tarifs ENJ 2026-27 (le règlement lu date de 2023-24).
- La couverture du Queyras par Skitourenguru, celle de Data-avalanche, et ses API.
- Une page Nordic France sur le SRN, des guides papier SRN, des clubs SRN locaux.
- La couverture réseau mobile aux Fonts et dans le Risoux. L'autonomie GPS de la Pixel Watch 2.
- La couverture de la Suisse par la couche swisstopo pentes > 30° n'est connue que par son nom.

Les données brutes (réponses Overpass, Camptocamp, Biodiv'sports, contours des massifs) sont dans `research/raw/srn/`.
