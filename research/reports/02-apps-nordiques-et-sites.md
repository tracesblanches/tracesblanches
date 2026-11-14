# Applications et services nordiques pour Cervières (Hautes-Alpes) et Chapelle-des-Bois (Doubs), et dates de vacances 2027

**En bref :**
- **Aucune application nordique spécialisée ne couvre les deux sites avec un suivi GPS du damage.** Nordic Pulse et Sporet font ce suivi, mais ni l'une ni l'autre ne couvre ces sites.
- **La seule source qui donne l'état de chaque piste pour les deux sites est la base Nordic France.** Elle contient l'ouvert/fermé par piste et la date du dernier damage par secteur. Elle est accessible par un widget web.
- **À Chapelle-des-Bois, l'Espace Nordique Jurassien (ENJ) publie en plus une API Geotrek ouverte.** Elle fournit l'état ouvert/fermé de chaque piste, les traces GPX/KML et des données mises à jour aujourd'hui. Pour une application maison, c'est la meilleure source.
- **Dates : la première semaine du congé belge (22–28 février) tombe pendant les vacances des zones A et B à la fois**, donc forte affluence.
- **Rien n'est prévu pour Wear OS.** Aucune fiche Play des applications nordiques étudiées n'en parle.

---

## 1. Applications et services nordiques

### 1.1 Tableau comparatif (fiches Google Play lues le 14/11/2026)

| Application / service | Prix Android | Cervières | Chapelle-des-Bois | Pistes nommées + couleur | Damage en direct | Hors ligne | Wear OS | Note Play |
|---|---|---|---|---|---|---|---|---|
| **Nordic Alpes du Sud** (`com.youstiti.nads`, éditeur Intence) | 0 € | Probable (réseau Nordic Alpes du Sud), non vérifié dans l'appli | Non | Non annoncé | Ouverture/fermeture des pistes seulement | Non annoncé | Non mentionné | 3,7 (55 avis), plus de 1 000 téléchargements, mise à jour le 12/10/2026 |
| **Nordic Pulse** (`com.nordicpulse.skier`) | 0 € (achats intégrés) | **Non** | **Non** | Oui | **Oui, GPS sur les dameuses** | « offline maps » (nouveautés) | Non mentionné | Pas de note affichée ; plus de 10 000 téléchargements ; mise à jour le 11/11/2026 |
| **bergfex: Skiing & Tracking** (`com.bergfex.mobile.android`) | 0 € (achats intégrés) | Page sans liste de pistes | Liste de pistes avec km et technique (classique/skating) | Noms oui, couleurs non | Colonne « Status » (vide hors saison) | Non annoncé pour le fond | Non mentionné | 4,9 (14 900 avis) |
| **OnTheSnow / Skiinfo** (`com.skireport`) | 0 € | **Non** | **Non** (seulement « Source du Doubs-Mouthe », domaine alpin) | Non | Non | – | – | 4,6 (39 800 avis) |
| **Sporet** (Norvège, référence ; `com.geodata.skiforeningen`) | 0 € sur Play, mais un abonnement est évoqué (voir plus bas) | Non | Non | Oui | **Oui : on voit les dameuses bouger sur la carte** | Avis : « offline mode is useless » | Non mentionné | 2,0 (246 avis, Play Norvège) |
| **Løyper/Längdspår/Loipen** (`com.fmsnordic.loyperapp`) | 0 € | Non | Non (couvre Norvège, Suède, Suisse, Allemagne) | Oui | Oui, date du dernier damage par tronçon | Non annoncé | Non mentionné dans sa propre description | 4,4 (6 avis, Play Norvège) |
| **Cross-Country (official)**, Suisse (`ch.langlauf.mobile`) | 0 € | Non | Non (Suisse seulement) | – | « open trails, snow conditions » | – | – | Pas de note propre affichée |

Sources des fiches :
- [Nordic Alpes du Sud](https://play.google.com/store/apps/details?id=com.youstiti.nads). Description : « Ouvertures / fermetures des pistes • Visualisation des webcams • Push géolocalisés ».
- [Nordic Pulse](https://play.google.com/store/apps/details?id=com.nordicpulse.skier). Nouveautés : « offline maps ».
- [bergfex](https://play.google.com/store/apps/details?id=com.bergfex.mobile.android)
- [OnTheSnow](https://play.google.com/store/apps/details?id=com.skireport)
- [Sporet](https://play.google.com/store/apps/details?id=com.geodata.skiforeningen). Description : « You see the piste machines moving on the map ».
- [Løyper](https://play.google.com/store/apps/details?id=com.fmsnordic.loyperapp)
- [Langlauf CH](https://play.google.com/store/apps/details?id=ch.langlauf.mobile)

Pour Wear OS, j'ai seulement cherché dans les descriptions Play, sans rien trouver. Une éventuelle application montre non déclarée là-dedans n'est pas vérifiée.

### 1.2 Vérifications de couverture

**Nordic Pulse.** J'ai extrait les 23 domaines français de [nordic-pulse.com/ski-areas](https://nordic-pulse.com/ski-areas) :
- Les voisins de nos sites y sont : `FR-HA-DomaineNordiqueDarvieux` (Arvieux, juste de l'autre côté du col d'Izoard) et `FR-BFC-LesRousses`.
- **Ni Cervières ni Chapelle-des-Bois n'y figurent.**
- Fonctionnement, d'après le site : « the app syncs the data to our server every few seconds… marking each trail with the precise date and time that it was last groomed » ([nordic-pulse.com](https://nordic-pulse.com)).

**Skiinfo.** La liste des stations [Jura](https://www.skiinfo.fr/jura/bulletin-neige) et [Alpes du Sud](https://www.skiinfo.fr/alpes-du-sud/bulletin-neige) ne contient ni Cervières ni Chapelle-des-Bois. Les URL testées pour ces deux sites renvoient « Page introuvable » (erreur 404).

**bergfex.**
- [Val de Mouthe / Chapelle des Bois](https://www.bergfex.com/grand-est/langlaufen/val-de-mouthe/loipen/) : « 44 classic cross-country skiing trails 320 km ». La liste par piste donne par exemple « Chaumoz Classical/Skating 10 km », « Greffier 14 km ».
- [Cervières](https://www.bergfex.com/provence-alpes-cote-d-azur/langlaufen/cervieres/) : texte seulement (« 42km of trails »), sans liste de pistes.

**Skisporet.** `skisporet.no` redirige (301) vers `https://www.sporet.no/` : le service a été absorbé par Sporet (vérifié avec curl). Selon [Alpinco](https://www.alpinco.com/en/kvitfjell/new-and-free-solution-trail-information) : « Skisporet is now being closed down », et Sporet « requires payment per user ». Cet article n'est pas daté (© 2025) ; le tarif actuel de Sporet n'est pas vérifié.

**Nordic App (Nordic France + Resalocal).** C'est **l'outil de saisie des pisteurs, pas une application grand public**. Dossier de presse de Haute-Savoie Nordic 2026-27 ([PDF](https://hautesavoiemontblanc-tourisme.com/app/uploads/hautesavoiemontblanc/2026/10/261001-HSN-Dossier-de-presse-2026-2027_Page.pdf)) : « Chaque jour, les pisteurs des domaines nordiques de France renseignent un outil commun appelé "Nordic App"… pistes ouvertes, météo, conditions de glisse, damage… pour l'hiver 2026-2027… cartes des sites, tracés GPX ». La nouvelle version sera « Disponible dans l'appli HSN et sur le site web d'HSN fin novembre 2026 ». C'est donc pour la Haute-Savoie, pas pour nos deux sites. Rien n'indique de suivi GPS des dameuses : c'est une saisie déclarative.

**Widget Nordic France (même base de données) : c'est la source qui sert vraiment.** Le script est `https://www.nordicfrance.fr/widgets/widget.js` et les points d'accès sont de la forme `/widget/marqueblanche/station/{id}`.
- **Cervières, id 3167**, trouvé dans le code de la page [nordicalpesdusud.com/domaines/cervieres](https://www.nordicalpesdusud.com/domaines/cervieres) : `generateBulletinStation(3167)`. La réponse actuelle de `https://www.nordicfrance.fr/widget/marqueblanche/station/3167` : « Dernier damage : 05/04 … Mise à jour : 06/04 PISTES 0 / 95km ». Les secteurs IZOARD, LES FONTS et LE LAUS sont listés avec le statut de chaque piste (L'Izoard 16 km, L'Aigalan 8, Le Sioux 6, Le Gibera 1,5, Le Savare 2,5, Le Cancel 3,5, La Pinatelle 7,5, plus les itinéraires « Raquettes Piéton » : les Fonts 22 km, les Oules 6 km).
- **Chapelle-des-Bois, id 4774**, trouvé dans le widget de l'association `espace-nordique-jurassien` : `…/association/station/4774/espace-nordique-jurassien` renvoie « Dernier damage : 25/02 … Mise à jour : 08/03 PISTES 0 / 231.2km ». Les 15 pistes de fond et les raquettes y sont avec leur km.
- Ces dates sont celles de la saison passée. **La mise à jour n'est pas en temps réel** : il y a une seule date de dernier damage par secteur, et je n'ai pas pu vérifier la fréquence en saison.
- La page [nordicfrance.fr/le-bulletin-neige](https://www.nordicfrance.fr/le-bulletin-neige) n'a pas répondu (délai dépassé, deux fois) : non vérifié.

**Snowmap de l'ENJ (Geotrek) : la plus intéressante pour Chapelle-des-Bois.**
- [snowmap.espacenordiquejurassien.com](https://snowmap.espacenordiquejurassien.com/search?practices=1,2&districts=8) est une application web installable (fichier `manifest.json`, `"display": "standalone"`). Elle a une rubrique « Contenus hors ligne » et des liens GPX, KML et PDF par piste.
- L'API publique est `https://admin.snowmap.espacenordiquejurassien.com/api/v2/`. J'ai interrogé `trek/?cities=14` : **42 itinéraires** (27 de ski de fond, 14 de raquettes, 1 piste multi-activités).
- Champs utiles :
  - `route` (1 = Fermé, 2 = Ouvert) ;
  - `difficulty` (facile, moyenne, difficile, très difficile) ;
  - `labels` (par exemple « Marquée partiellement ouverte le 13/01/2025 à 15h15 ») ;
  - `networks` (« Itinéraire Grande traversée du Jura ») ;
  - `update_datetime` : **« 2026-11-14T15:15:09 » pour la plupart des pistes, c'est-à-dire aujourd'hui.**
- Ces mises à jour à heure fixe laissent penser à une synchronisation automatique de l'état (déduction de ma part, non vérifiée). Dans les labels, les heures se répètent (10h03, 15h15, 08h30). Aujourd'hui, en novembre, deux pistes (« Le Glacier », « Les Bornes ») sont marquées « Ouvert ». Ce champ n'est donc pas forcément fiable hors saison.
- Le « Mon Pass Nordic ENJ » est une application de forfait ([macommune.info, 2020](https://www.macommune.info/ski-nordique-decouvrez-la-nouvelle-application-mon-pass-nordic-enj)) : hors sujet.

**Non trouvés ou non vérifiés :**
- Skitrax : aucune application trouvée.
- « Fluo » : aucune application trouvée.
- SNOWsat : aucune application grand public sur Play (recherche « snowsat »).
- Loipen-Info, Loipenportal, Tourenwelt, Skinfo, Snowheads (forum), AllTrails en hiver : non examinés en détail.

---

## 2. Les deux sites

### 2.1 Cervières : domaine nordique Cervières-Izoard (réseau Nordic Alpes du Sud)

| Élément | Constat | Source |
|---|---|---|
| Kilométrage | Les chiffres divergent : « 53 km de pistes en techniques alternatif et skating » (mairie), « 42 kilomètres répartis en deux secteurs » (Nordic Alpes du Sud). Le widget Nordic France donne 95 km, raquettes comprises. Somme des boucles de fond dans le widget : environ 45 km. | [cervieres.fr](https://cervieres.fr/domaine-nordique-2/domaine-nordique/), [nordicalpesdusud](https://www.nordicalpesdusud.com/domaines/cervieres) |
| Secteurs | Le Laus et la haute vallée des Fonts. **Les Fonts : « ouvert uniquement à partir des vacances de février » → fermé la première semaine de janvier.** | [cervieres.fr](https://cervieres.fr/domaine-nordique-2/domaine-nordique/) |
| Col d'Izoard | « A compter de la saison 2026/2027, la piste du col d'Izoard deviendra un itinéraire en accès libre avec un damage très limité » | idem |
| Piétons et raquettes | « 46 km d'itinéraires pour piétons et raquettes ». Trois itinéraires multi-activités damés : Izoard, Les Oules, Les Fonts. | idem ; [hautesvallees.com](https://www.hautesvallees.com/lizoard/que-faire/en-hiver/domaine-nordique-de-cervieres/) |
| Ouverture 2026-27 | « A partir du 19 décembre 2026 jusqu'au 31 mars 2027 », fermeture à 16h en semaine en janvier | [cervieres.fr](https://cervieres.fr/domaine-nordique-2/domaine-nordique/) |
| Bulletin de damage | Tableau HTML à la main sur [cervieres.fr/…/info-damage](https://cervieres.fr/domaine-nordique-2/info-damage/), avec les colonnes « Dernier DAMAGE / ETAT CLASSIC / ETAT SKATING ». Il y a aussi le widget Nordic France (id 3167) sur Nordic Alpes du Sud et la chaîne WhatsApp « NORDIC ALPES DU SUD COMMUNITY ». | idem |
| Plans en PDF | Plan sur Nordic France : [Plan-des-pistes-Cervieres.pdf](https://www.nordicfrance.fr/content/uploads/2018/12/Plan-des-pistes-Cervieres.pdf). Dépliant de la mairie : [Nouveau-depliant-Cervieres-2022_comp.pdf](https://cervieres.fr/wp-content/uploads/sites/268/2022/06/Nouveau-depliant-Cervieres-2022_comp.pdf). | liens trouvés dans les pages |
| Webcams | Viewsurf : `https://pv.viewsurf.com/338/Cervieres` et `/1026/Cervieres` (iframes de la page Nordic Alpes du Sud) | [nordicalpesdusud](https://www.nordicalpesdusud.com/domaines/cervieres) |
| Réseau plus large | Domaine membre de Nordic Alpes du Sud. Saison « Massif 200 € / Cervières 75 € ». Liaison par le col vers le Queyras (Arvieux, couvert par Nordic Pulse). | idem |

### 2.2 Chapelle-des-Bois : Espace Nordique Jurassien, secteur « Haut Doubs – Mouthe – Chapelle des Bois »

| Élément | Constat | Source |
|---|---|---|
| Structure | ENJ, district 8 de la Snowmap (« Haut Doubs - Mouthe - Chapelle des Bois »). Gestion par la Communauté de communes des Lacs et Montagnes du Haut-Doubs (« Lacs et Montagnes du Haut Doubs : Des Fourgs à Chapelle des Bois »). Office de tourisme du Val de Mouthe Chapelle des Bois. | API ENJ `informationdesk/6`, [page webcams ENJ](https://www.espacenordiquejurassien.com/webcams.html) |
| Kilométrage | « 14 Track (that is 127 km) » pour le fond (3 vertes, 6 bleues, 4 rouges, 1 noire) ; raquettes : « 7 Track (that is 62 km) ». **127 km correspond à la somme des boucles** : leurs km Nordic France additionnés font 127,5. | [montagnes-du-jura.fr](https://en.montagnes-du-jura.fr/nordic-areas-and-resorts/site-nordique-de-chapelle-des-bois) |
| Grande Traversée du Jura | Étapes « GTJ ski 12 : Le Pré Poncet - Chapelle des Bois », « GTJ ski 13 : Chapelle des Bois - Bellefontaine », « GTJ raquettes 05 et 06 » | API ENJ (`networks:[1]`) |
| État des pistes | Widget Nordic France (station 4774) sur [espacenordiquejurassien.com/bulletin-neige.html](https://www.espacenordiquejurassien.com/bulletin-neige.html), plus le champ ouvert/fermé de la Snowmap et de son API | voir 1.2 |
| Webcam | `https://app.webcam-hd.com/espace-nordique-jurassien/val-de-mouthe_chapelle-des-bois` | [ENJ webcams](https://www.espacenordiquejurassien.com/webcams.html) |
| Application officielle | Aucune application native de suivi. La Snowmap est une application web installable. | – |

### 2.3 OpenStreetMap (Overpass, données du 14/11/2026 vers 18h08 UTC)

Requête pour Cervières (le cadre a été élargi, puis filtré au nord du col d'Izoard, latitude ≥ 44,820, pour exclure Arvieux et Brunissard côté Queyras) :
```
[out:json][timeout:120];(way["piste:type"~"nordic|hike|snowshoe|skitour|sled"](44.77,6.66,44.90,6.86);relation["piste:type"~"nordic|hike|snowshoe"](44.77,6.66,44.90,6.86););out body geom;
```
Requête pour Chapelle-des-Bois :
```
[out:json][timeout:120];(way["piste:type"](46.54,6.02,46.68,6.20);relation["piste:type"](46.54,6.02,46.68,6.20););out body geom;
```

| | Cervières (côté nord de l'Izoard) | Chapelle-des-Bois (cadre large, inclut Bellefontaine, Chaux-Neuve, Bois-d'Amont et une partie suisse) |
|---|---|---|
| Ways `piste:type=nordic` | 46 ways balisés directement. Avec les membres des relations, sans doublons : **115 ways, environ 37,7 km** (dont 34,5 km nordique et 3,2 km « hike ») | 598 ways, environ 270 km dans tout le cadre |
| Relations | La Pinatelle (noire, 7,8 km), Le Cancel (rouge), Le Gibera (verte), Le Savare (bleue), Izoard (7,6 km), L'Aigalan, Le Chemin du Sioux (13 km), Liaison Le Laus - Les Fonts, plus une relation `hike` « Route du Col d'Izoard » (11,1 km) | 84 relations nordic et 3 relations `hike`. **Les 13 pistes de Chapelle testées sont toutes présentes**. Union de leurs ways : 65,7 km (contre 127,5 km de boucles officielles cumulées) |
| `piste:name` | 0 sur les ways ; noms portés par `name` (32 ways) et par les relations | 0 sur les ways ; 17 relations avec `piste:name`, 69 avec `name` |
| `piste:difficulty` | 35 ways sur 46 ; relations : 6 sur 16. Couleur `colour` sur 15 relations sur 16. | 577 ways sur 655. **Incohérent** : les ways partagés sont surtout « easy » même sous Greffier (noire). Il vaut mieux lire la difficulté dans le `colour` des relations (84 sur 87). |
| `piste:grooming` | 37 ways sur 46 (`classic;skating`) | 553 ways sur 598 ; `piste:oneway` sur 263 ways |
| Raquettes | Pas de `snowshoe`. 16 ways `nordic;hike` et 8 ways `hike` dans le cadre élargi. | Pas de `snowshoe`. Relations `hike` : Chalet Gaillard, La Roche Bernard, Tour des Lacs (3, contre 14 itinéraires raquettes ENJ) |
| Manques | Le secteur des Fonts n'est cartographié qu'en partie (environ 2,6 km de ways balisés directement) | Absentes : Chez Michel, La Morberande, Tour du village et la piste éclairée en tant que relation (sur les 27 itinéraires de fond ENJ) |

---

## 3. Dates

| Calendrier | Dates | Source |
|---|---|---|
| **Fédération Wallonie-Bruxelles, congé de détente** | « Du lundi 22 février 2027 au dimanche 7 mars 2027 » | [enseignement.be](http://www.enseignement.be/index.php?page=23953) ; confirmé par [jeminforme.be](https://www.jeminforme.be/conges-scolaires) et [CSC](https://www.lacsc.be/vos-droits/travailler-dans-lenseignement/ma-carriere-travailler-dans-lenseignement-calendrier-scolaire) (« au vendredi 5 mars ») |
| Fédération Wallonie-Bruxelles, vacances d'hiver | « Du lundi 21 décembre 2026 au dimanche 3 janvier 2027 » | enseignement.be |
| **France, zone A (académie de Besançon)** | Hiver : « Du samedi 13 février 2027 au lundi 1er mars 2027 » | [service-public.gouv.fr](https://www.service-public.gouv.fr/particuliers/actualites/A18563) (arrêté du 22/10/2025) |
| Zone B (dont Aix-Marseille, donc Cervières ; aussi Lille, Nancy-Metz, Strasbourg) | « Du samedi 20 février 2027 au lundi 8 mars 2027 » | idem |
| Zone C | « Du samedi 6 février 2027 au lundi 22 février 2027 » | idem |
| France, Noël (toutes zones) | « Du samedi 19 décembre 2026 au lundi 4 janvier 2027 » | idem |

**Affluence à Chapelle-des-Bois :**
- **Semaine du 22 au 28 février** : zones A et B en vacances en même temps, donc pic probable.
- **Semaine du 1er au 7 mars** : zone B seulement.
- Les vacances scolaires suisses (Vaud) n'ont pas été vérifiées.

**Première semaine de janvier à Cervières :** affluence de Noël jusqu'au dimanche 3 janvier, reprise des cours le 4 janvier. Le secteur des Fonts sera fermé et l'Izoard ne sera qu'en accès libre, peu damé.

---

**Ce que je n'ai pas pu vérifier :**
- Le contenu réel de l'application Nordic Alpes du Sud pour Cervières.
- La fréquence de mise à jour du widget Nordic France en saison.
- La page bulletin de nordicfrance.fr (délai dépassé).
- La note Play de Nordic Pulse.
- Le tarif actuel de Sporet.
- La présence d'applications Wear OS non déclarées dans les descriptions Play.

Les fichiers bruts (JSON Overpass, réponses de l'API ENJ, pages Play) sont dans `research/raw/` (sous-dossiers `osm/`, `enj/`, `dl/`).
