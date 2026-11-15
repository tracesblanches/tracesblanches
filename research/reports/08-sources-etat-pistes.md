# Rapport 08 : sources d'état des pistes (ouvert / fermé, damage) pour les 219 domaines

Ticket G3 (#119). Enquête du 15/11/2026, faite hors saison : les états lus datent de l'hiver passé. Annexe : `08-sources-etat-pistes.csv`, une ligne par domaine (source retenue, URL, format, conditions, autorisation, niveau de preuve, autres sources).

## Chiffres clés

- **Source dont l'usage est autorisé : 0 domaine sur 219 (0 %).** Aucune source d'état n'affiche de licence ouverte. Les sources ouvertes (OpenSkiMap, SkiRadar, DATAtourisme, data.gouv) ne donnent pas l'état du jour des pistes de fond.
- **Couverts seulement avec autorisation : 151 sur 219 (69 %), soit 83 % des 4 415 km nordiques.**
  - 108 domaines (49 %, 66 % des km) avec une preuve solide : API ENJ (36) ou station présente dans le widget Nordic France (72).
  - 43 sont déduits : fiche Nordic France rapprochée par le nom (30), rapprochement incertain (6), Pyreneige (7).
- **Sans source d'état trouvée : 68 domaines (31 %, 17 % des km).** 29 font moins de 5 km : ce sont surtout des morceaux de domaines alpins. 39 font 5 km ou plus : le Sancy, le Cantal, des stations alpines, les Vosges du Sud, les Ardennes, le Haut-Folin et des petits sites du Doubs.
- **Un seul accord, celui de Nordic France, couvrirait 136 domaines (62 %, 75 % des km).** Avec celui de l'ENJ : 144 (66 %, 81 % des km).

## Sources examinées

| Source | Requête testée | Format | Couverture (domaines) | Fraîcheur constatée | Conditions (texte lu) | Autorisation |
|---|---|---|---|---|---|---|
| ENJ, API Geotrek Snowmap | `admin.snowmap.espacenordiquejurassien.com/api/v2/trek/?fields=id,route,…` (200, 641 itinéraires) ; `trek_route/` : `1 Fermé`, `2 Ouvert` ; `in_bbox=` fonctionne | JSON | 36 (Doubs, Jura, Ain) : 380 des 399 itinéraires de ski partent dans une enveloppe de domaine ou à moins de 1 km | `update_datetime` du 14/11/2026 à 15:15 ; en saison : non mesurée | « dispose de tous les droits de propriété sur tous ses éléments, affichés ou en bases de données » (espacenordiquejurassien.com/mentions.html) | oui (courrier 1) |
| Jura Outdoor (agrégateur Geotrek du CD39) | `geotrek-cd39-aggregator.makina-corpus.net/api/v2/trek/?practices=20,21,22,23` (271) | JSON | même contenu que l'ENJ, Jura seulement : `provider` vaut « ENJ » ; labels 56 « Fermé (pratiques hivernales) » et 57 « Ouvert (pratiques hivernales) » | 14/11/2026 | « Toute reproduction, même partielle, du site ou de l'un de ses éléments est interdite sans le consentement écrit et préalable » ; « données […] réservés » (jura-outdoor.com/information/3-Mentions-legales) | oui |
| Nordic France, widget `/widget/marqueblanche/station/{id}` | 3167 (Cervières, 200), 6413 (Beille, id lu sur la fiche `nordicfrance_station/station-de-beille`) | HTML | 72 domaines avec un id connu ; 36 autres par les 156 fiches du sitemap (id à lire sur chaque fiche) | « Mise à jour : 06/04 » (Cervières), « 29/03 » (Beille) ; la fréquence en saison n'est pas mesurée | « Toute reproduction, représentation, modification, publication, adaptation de tout ou partie des éléments du site […] est interdite, sauf autorisation écrite préalable » (nordicfrance.fr/mentions-legales, relu) | oui (courrier 2, #9) |
| Nordic Massif Central (Montagnes du Massif Central) | `nordic-massif-central.com/station.php?id=1` (200) : « Domaine fermé », état par piste (`nmc-dot ferme`) ; 19 stations | HTML | 8 domaines rapprochés, 5 incertains (tous aussi sur une fiche Nordic France) | « Le Mas de la Barque : 37 km de pistes ouvertes » le 15/11 | « Toute reproduction, même partielle, est soumise à autorisation préalable » (mentions-legales.php) | oui |
| Lumiplan, bulletin v3 | `bulletinv3.lumiplan.pro/bulletin.php?station=evasion-montblanc` (200) : 12 pistes de fond, `lp_runway_trail_*`, `*_groomed` ; `tignes` : aucune piste de fond ; `montgenevre`, `alpedhuez` : page vide | HTML | 2 domaines (Megève, déjà sur Nordic France) | « Mis à jour le 15/11/2026 à 07:56 » | lumiplan.com : « un usage personnel et non commercial » ; reproduction « sans consentement préalable et écrit, est illicite » (page du site institutionnel, pas du bulletin) | oui |
| Pyreneige | page `pyreneige.fr/ressources/api/` lue ; aucun point d'accès public | webservice | 11 domaines pyrénéens (couverture par station non vérifiée) | « constamment mis à jour » (déclaratif) | « connaître les tarifs du service API, contactez-nous » : **payante** (le rapport 03 la disait « publique ») | oui (contrat) |
| Alsace Destination Tourisme, PDF | `apps.tourisme-alsace.info/…/2001613000004_neige_lac_blanc.PDF` (200) : « Ski de fond 0 / 14 », pistes listées | PDF | 4 (Lac Blanc, Trois Fours, Markstein, Grand Ballon) | bulletin du 02/03/2026, `Last-Modified` du 08/03/2026 | visit.alsace : reproduction « strictement interdite sans autorisation écrite » sauf flux RSS ; je suppose que le même éditeur publie les PDF | oui |
| SkiRadar `api.skiradar.fr/api/v1/geo-feed` | flux du 14/11 relu | JSON | bloc `nordique` sans état (itinéraires OSM) ; `neige` présent pour 14 stations alpines seulement, compteurs Lumiplan | quotidien | « réutilisables avec attribution « skiradar.fr » » | non, mais pas d'état nordique |
| OpenSkiMap `ski_areas.csv` | en-tête lu (requête partielle) | CSV | `status` (cycle de vie : operating…) et `websites` | quotidien | ODbL | non, mais pas d'état du jour |
| data.gouv.fr | API : « ski de fond », « état des pistes », « enneigement stations » : 0 jeu ; dataservice « ski » : seulement SkiRadar | — | — | — | — | — |
| bergfex | `/frankreich/langlaufen/` (200), 161 domaines avec leurs km | HTML | non rapproché | — | « may not be copied or used, even in part, without authorisation » (AGB §7) | oui |
| Skiinfo | CGU lues | HTML | stations alpines (rapport 07) | — | « réservée à un usage strictement personnel » (§4) | oui |
| skiresort.info | CGU lues | HTML | pas d'état des pistes de fond (seulement « Open slopes / lifts ») | — | « © Skiresort Service International GmbH. All rights reserved. » | oui |
| OpenSnow | conditions de l'API lues | API | prévisions météo, pas d'état des pistes trouvé | — | « limited, non-exclusive, non-transferable, revocable license » ; revente ou redistribution soumise à « prior written consent » | oui |

## Instances Geotrek

- Liste officielle : `utilisateurs.geotrek.fr/clients_map.geojson` (81 instances, avec leur emprise). Les 219 points de domaine tombent tous dans au moins une emprise (FFC et AURA sont nationales ou régionales) ; 33 instances recoupent au moins un domaine.
- J'ai interrogé `trek_practice/`, `trek_route/` et `label/` sur 21 API, l'URL étant lue dans `apiUrl` de chaque site Geotrek-rando :
  - **Alpes** : CD05 (agrégateur), CD04, Écrins, Vanoise (et Valmorel), Mercantour, Bauges, Vercors, CEN Haute-Savoie, Chemins des Parcs ;
  - **Jura** : CD25 (Doubs), CD39, PNR Haut-Jura ;
  - **Massif central** : Volcans d'Auvergne, Haute-Loire, Livradois-Forez, Pilat, Grands Causses et Aubrac, Cévennes ;
  - **Pyrénées** : CD66, CD65 (Gavarnie) ;
  - **non lisible** : CD64 (`geotrek-admin.le64.fr`, HTTP 403).
- **Seule l'ENJ publie un état**, en détournant le champ `route` (1 Fermé, 2 Ouvert). Le CD39 le republie sous forme de labels. Partout ailleurs, `route` vaut Boucle, Aller-retour, etc. Le Vercors a une pratique « Nordique » (9 itinéraires), mais `route` y vaut Boucle ou Aller-retour.
- Ce n'est pas un manque de version :
  - le modèle Geotrek-admin (2.127.0 du 03/11/2026, `geotrek/trekking/models.py`) n'a aucun champ d'ouverture ;
  - `Route` est une liste libre (« Name », 128 caractères) ;
  - `impracticable` (présent dans l'API du Vercors, toujours `False`) vient des zones de vigilance (`geotrek/zoning/models.py` : « Tick this box if the terrain is impracticable due to the vigilance »). Ce n'est pas un état de damage.
- Les instances sans site déployé (Ariège, Lozère, Hautes-Pyrénées) n'ont pas été trouvées. Je n'ai pas cherché d'instances absentes de la carte officielle.

## Par massif (CSV pour le détail)

| Massif | Domaines | Meilleure source | Sans source |
|---|---|---|---|
| Jura, Doubs, Ain | 48 | ENJ 36, Nordic France 2 (widget 1, fiche 1) | 10 (Bolandoz, Le Crouzet, Les Premiers Sapins ×2, Malbuisson-La Fuvelle, Arçon, Le Bélieu, Brénod, Menthières, Grande-Rivière) |
| Alpes du Nord (73, 74, 38) | 79 | Nordic France 50 (widget 43, fiche 7) | 29 : surtout des morceaux de grands domaines alpins (3 Vallées, Portes du Soleil, Alpe d'Huez, Tignes, La Plagne), plus Espace Diamant, Col du Feu, Saint-François-Longchamp, Aillon-Margériaz, Vallorcine, Thollon |
| Alpes du Sud (04, 05, 06, 26) | 37 | Nordic France 34 (widget 27, fiche 7 dont Saint-Agnan-en-Vercors, incertaine) | 3 (Caille, Montgenèvre, Chabanon) |
| Massif central (07, 12, 15, 19, 42, 43, 48, 63) | 28 | fiche Nordic France 13 (dont 5 incertaines), NMC en second pour les 13 | 15 (Sancy : Super Besse, Pessade, Guéry, Pertuyzat, La Tour-d'Auvergne ; Cantal : Légal, Falgoux, Laveissenet, Cézens ; Prabouré ; Saint-Setiers ×3 ; Croix de Bauzon ; Lachamp-Raphaël) |
| Vosges, Alsace, Haute-Saône | 11 | fiche Nordic France 5, PDF Alsace en second pour 4 | 6 (Ventron, Rouge Gazon, Cornimont, Plancher-les-Mines, La Montagne, Le Hohwald) |
| Pyrénées (09, 64, 65, 66) | 11 | Nordic France 4 (dont Beille, vérifié), Pyreneige 7 | 0 |
| Ardennes, Morvan | 5 | — | 5 |

## Vérifié et déduit

- **Vérifié par requête** :
  - les réponses et champs cités dans le tableau, le rattachement des itinéraires ENJ aux enveloppes (point de départ dans le polygone ou à moins de 1 km) ;
  - les 81 instances Geotrek et les 21 API sondées ;
  - les widgets 3167 et 6413, la page NMC id=1, les bulletins Lumiplan et le PDF Alsace testés ;
  - les textes de conditions cités.
- **Déduit** :
  - pour les 72 domaines rattachés au widget, le lien repose sur le nom : 66 viennent de `nordic_france.json` (P3), 1 est Beille (vérifié), et 5 sont des rapprochements manuels (Ratery = « Ratėry 1700 », Agy = Saint-Sigismond, Orange = La Roche-sur-Foron, Coulmes = Rencurel, Alpe du Grand Serre = La Morte) ;
  - les 36 fiches du sitemap rapprochées par le nom (30, plus 6 incertaines) n'ont pas été ouvertes, donc leur id de widget et leur état ne sont pas vérifiés ; Beille est la seule fiche ouverte et testée ;
  - les 13 domaines NMC sont rapprochés par le nom ;
  - pour Pyreneige, la liste des stations couvertes n'est pas publiée.
- **Non vérifié** :
  - la fréquence de mise à jour en saison, pour toutes les sources (tout est figé depuis mars-avril, sauf les horodatages ENJ du 14/11 et Lumiplan du 15/11) ;
  - les conditions propres à `bulletinv3.lumiplan.pro` (pas de mentions sur la page ; `robots.txt` en 404) et aux PDF d'Alsace ;
  - les sites des stations des 68 domaines sans source (conditions non lues) ;
  - Nordic Pulse, Apidae et DATAtourisme (pas relus ici : le rapport 03 dit « périodes d'ouverture seulement ») ;
  - le contenu de l'appli Haute-Savoie Nordic (annoncée pour fin novembre 2026).

## Conséquences pour #119 et #9

1. L'accord écrit de **Nordic France** (#9, courrier 2) est le levier principal : 136 domaines, 75 % des km. Il faudrait demander un accès à la base « Nordic App » plutôt qu'au HTML.
2. L'accord de l'**ENJ** (courrier 1) couvre le Jura en JSON. Le CD39 republie les mêmes données, donc il faut l'accord de l'ENJ, voire de Jura Tourisme aussi.
3. En complément : **Montagnes du Massif Central**, un devis **Pyreneige**, **Alsace Destination Tourisme**.
4. Pour les 68 autres domaines, et tant qu'aucun accord n'est obtenu, le pack affiche seulement un lien vers le bulletin officiel (URL `websites` d'OpenSkiMap quand elle existe) ; le code ne doit jamais deviner un état.
