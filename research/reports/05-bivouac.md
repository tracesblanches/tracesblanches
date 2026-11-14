# Bivouac hivernal à Cervières et à Chapelle-des-Bois : règles et données ouvertes (état au 14/11/2026)

**En bref :**
- **Cervières :** le bivouac est toléré par défaut, avec l'accord du propriétaire. Le feu est interdit sur toute la commune, réchauds compris. Il faut éviter quatre zones : le village, la Casse Déserte, les abords des monuments historiques et les éventuels captages d'eau.
- **Chapelle-des-Bois :** une grande partie du secteur est sous protection. Trois arrêtés préfectoraux de protection de biotope (APPB) y interdisent le bivouac en hiver ou toute l'année. Le reste du fond de vallée est un site inscrit, où le camping isolé est interdit sauf dérogation.
- **Côté suisse :** le camping occasionnel demande l'accord du propriétaire. Le district franc du Noirmont interdit de camper.

## 1. Droit général français

| Point | Règle | Source ouverte |
|---|---|---|
| Bivouac ou camping | Le Code de l'urbanisme ne définit pas le bivouac. Une réponse ministérielle de 2020 dit que « les décrets différencient le campement et le bivouac ». Elle ajoute qu'il n'existe « pas de règle générale en matière de campement en montagne » et renvoie à R111-32. | [AN QE 21803](https://questions.assemblee-nationale.fr/dyn/15/questions/QANR5L15QE21803.pdf) |
| Définitions dans des textes locaux | Arrêté du préfet de l'Ain, 2017 (réserve de la Haute Chaîne du Jura) : bivouac = « campement d'une nuit en pleine nature, constitué d'une installation légère et temporaire ». Arrêté municipal d'Arvieux 2023-22 : tente où l'on ne tient pas debout, montée après 18 h, démontée avant 9 h, une seule nuit par secteur. | [copie AP 2017](https://binnette.github.io/GAC/Bivouac/assets/2017_Jura_AP-bivouac-signe.pdf) (copie tierce), [AM Arvieux](https://www.pnr-queyras.fr/ecmedias/2023/08/Arvieux_2023-22-Reglementat°-du-bivouac.pdf) |
| R111-32 (en vigueur depuis 2016) | Camping libre « hors de l'emprise des routes et voies publiques », « avec l'accord de celui qui a la jouissance du sol », sous réserve de « l'opposition du propriétaire ». | [Légifrance, sous-section Camping](https://www.legifrance.gouv.fr/codes/section_lc/LEGITEXT000006074075/LEGISCTA000031719402/) |
| R111-33 (en vigueur depuis 01/04/2017) | Camping isolé interdit, sauf dérogation : 1° rivages et sites inscrits ; 2° sites classés ou en instance de classement ; 3° sites patrimoniaux remarquables (SPR) et abords des monuments historiques ; 4° rayon de 200 m autour des captages d'eau potable. Il n'y a plus de clause sur les « bois classés ». Jura Tourisme l'affirme encore, ce qui semble dépassé. | [Légifrance R111-33](https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000034355031) |
| R111-34 | Interdictions possibles par le plan local d'urbanisme (PLU) ou par arrêté du maire. Elles ne s'appliquent qu'après affichage en mairie et « apposition de panneaux aux points d'accès habituels ». | idem |
| Zone grise | Aucun texte ne dit si un bivouac d'une nuit est du « camping pratiqué isolément » au sens de R111-33. Lecture prudente pour l'app : le traiter comme interdit en site inscrit, site classé, SPR et abords de monument historique. | — |
| Feu en forêt | L131-1 : défendu à toute personne autre que le propriétaire « jusqu'à une distance de 200 mètres des bois et forêts ». Amende de 4e classe (R163-2). L'article ne mentionne pas les réchauds. | [L131-1](https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000025245867), [R163-x](https://www.legifrance.gouv.fr/codes/section_lc/LEGITEXT000025244092/LEGISCTA000026127977/) |
| Camping en forêt | Seul R163-11 réprime le camping, et uniquement en **forêt de protection** (5e classe). Je n'ai trouvé aucun texte national qui interdise le bivouac en forêt domaniale : l'ONF fixe des règles forêt par forêt. Le GPU ne montre aucune servitude « forêt de protection » (A7) dans les deux zones. En forêt communale, le propriétaire est la commune, donc il faut son accord (R111-32). | idem, [FAQ ONF Île-de-France](https://www.onf.fr/vivre-la-foret/%2B/2a7a::faq-agence-ile-de-france-ouest.html) |
| Hautes-Alpes, feu | Arrêté « période rouge » du 23/07/2026 : tout feu interdit à moins de 200 m des bois et landes (barbecues, feux de camp, places à feu). | [AP 05-2026-07-23-00002](https://www.pnr-queyras.fr/ecmedias/2026/07/Arrete-prefectoral-periode-rouge-emploi-du-feu-2026.pdf) |
| Hiver | Je n'ai trouvé aucun texte propre à l'hiver (igloo, trou à neige). Les règles saisonnières viennent des APPB (15/12–30/06). | — |

## 2. Espaces protégés : contrôle par intersection (WFS Géoplateforme, calcul shapely)

Les positions des villages sont des coordonnées approximatives que j'ai saisies moi-même. Le pourcentage indique la part de la zone de recherche couverte.

**Cervières (zone 44.77,6.66 → 44.92,6.86)**
- **Aucun parc national, aucune réserve naturelle (nationale ou régionale), aucun APPB.** Le parc national des Écrins n'intersecte pas la zone.
- **PNR du Queyras :** 28,8 % de la zone (côté Arvieux). **Cervières n'en fait pas partie.** Le décret n° 2026-304 du 21/04/2026 classe dix communes et Cervières n'y figure pas ([AIDA](https://aida.ineris.fr/reglementation/decret-ndeg-2026-304-210426-portant-renouvellement-classement-parc-naturel-regional)).
- **Sites classés :** Casse Déserte (FR3200828, 0,4 %, côté sud de l'Izoard, Arvieux) et Vallée de la Clarée (0,2 %, extrême nord).
- **Site inscrit :** Ville vieille de Briançon.
- **SPR de Cervières :** environ 3,3 km², il contient le village. R111-33 3° s'y applique.
- **Natura 2000 :** Rochebrune–Izoard–Cerveyrette (FR9301503, 54 %), ainsi que Steppique Durancien, Clarée, Haut Guil et la ZPS Bois des Ayes.
- **Autres :** réserve biologique dirigée du Bois des Ayes (FR2300113, 0,9 %, commune de Villar-Saint-Pancrace), réserve de biosphère du Mont Viso.

**Chapelle-des-Bois (zone 46.54,6.02 → 46.68,6.20)**
- **Aucun parc national, aucune réserve naturelle (nationale ou régionale).**
- **APPB :**
  - « Forêts d'altitude du Haut-Doubs » (FR3801178, créé le 31/12/2025) : environ 1 123 ha dans la zone.
  - « Forêts d'altitude du Haut-Jura » (FR3800988, 2019) : environ 938 ha, le Risoux côté Bellefontaine / Bois-d'Amont.
  - « Forêt du Paradis » (FR3800137, 1986) : environ 637 ha.
  - Plus « Écrevisse à pattes blanches (39) » et les deux APPB « Corniches calcaires » (Doubs et Jura).
- **PNR du Haut-Jura :** 80 % de la zone.
- **Site inscrit « Site de Chapelle-des-Bois » :** 3 971 ha, 02/05/1974. Il contient le village, le lac des Mortes et le Chalet Gaillard ([liste DREAL](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/liste-completesi-pourweb.pdf)).
- **Site classé :** Haute vallée de la Saine (2,3 %).
- **Natura 2000 et Ramsar :** Risoux, Mont-d'Or/Noirmont/Risol, Tourbières de Chapelle-des-Bois–Bellefontaine, Entrecôtes du Milieu, Vallée de l'Orbe, site Ramsar FR7200019.
- **Mont d'Or (hors zone) :** seulement les APPB Corniches et Écrevisse (25). Pas d'APPB grand tétras.

**Règles de bivouac dans ces espaces**

| Espace | Règle bivouac | Source |
|---|---|---|
| APPB Haut-Jura 2019 (Risoux, Massacre, Ban-Arobiers) | « Le bivouac, avec ou sans tente, est interdit du 15 décembre au 30 juin. » La randonnée hors des itinéraires autorisés est interdite sur la même période. Feu interdit hors des lieux prévus. | [motivations](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/appb_motivations_cle18f7bf.pdf), [projet art. 7](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/projet_appb_forets_altitude_avec_annexes_consultations_cle5134bf.pdf), [plaquette](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/plaquette_appb_forets_altitude_haut_jura_cle73afd4.pdf) |
| APPB Haut-Doubs (projet d'août 2025 ; arrêté signé le 31/12/2025, texte final non trouvé) | **Sur tout le périmètre (art. 2) :** sortie des itinéraires balisés interdite du 15/12 au 30/06. **Zones cœur** Risol nord et Risol sud (Mouthe), Verdet (Chaux-Neuve) : camping, bivouac et feu interdits toute l'année (art. 11) ; accès interdit la nuit (art. 6) ; accès interdit du 15/12 au 30/06 (art. 9). Itinéraires ajoutés après consultation : la Jaïque, la Citadelle → Chalet Griffon. | [projet](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/appb_forets_altitude_haut-doubs_v25-08-2025.pdf), [motif de la décision](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/3_motif_de_la_decision_appb_forets_altitude_du_haut-doubs.pdf), [signature](https://groupe-tetras-jura.org/2026/06/05/un-nouvel-appb-pour-les-forets-daltitude-du-haut-doubs/) |
| APPB Forêt du Paradis (AP n° 111 du 10/02/1986, Foncine-le-Haut et voisines) | Art. 3 : « camping et bivouac interdits sur toute l'étendue de la réserve », sauf sur les parcelles B771 et B775 du 1/7 au 30/9. Feu en forêt interdit du 15/2 au 31/5. | [AP 1986](https://www.bourgogne-franche-comte.developpement-durable.gouv.fr/IMG/pdf/114_39_APB_Foret_Paradis_AP_19860210_cle1da5ef.pdf) |
| PNR | Pas de règle opposable aux particuliers. Les règles du Queyras sont des **arrêtés municipaux** pris par les maires (visa des articles L2212-1 et 2 du Code général des collectivités territoriales). Haut-Jura : bivouac « toléré » selon une page touristique. | [AM Arvieux](https://www.pnr-queyras.fr/ecmedias/2023/08/Arvieux_2023-22-Reglementat°-du-bivouac.pdf), [Jura Tourisme](https://www.jura-tourism.com/vivre-le-jura/pratiquer/balades-randonnees-pedestres/balisage-cartoguides-pour-randonner/bivouac-dans-le-jura/) |
| Natura 2000 | Aucune interdiction de bivouac dans les textes consultés. | non vérifié par texte primaire |

## 3. Règles locales

**Cervières**
- **PLU du 29/07/2021 :** une recherche « camping » dans le règlement écrit ne donne aucune interdiction du camping isolé (seulement des mentions de « camping à la ferme »).
- **Arrêté municipal sur le bivouac :** aucun trouvé.
- **Feu :** « L'usage du feu, y compris via des réchauds et barbecues, est totalement prohibé sur la commune de Cervières (08/07/2022) » ([PNR Queyras](https://www.pnr-queyras.fr/bivouac/)). Je n'ai pas vu le texte de cet arrêté.
- **Arvieux (versant sud de l'Izoard) :** arrêté municipal 2023-22. Bivouac sous tente toléré pour une randonnée itinérante, à plus d'une heure de marche du village et des hameaux. Une nuit par secteur, entre 18 h et 9 h. Interdit près des cabanes et chalets sans accord.
- **Domaine nordique de Cervières :** aucune règle de bivouac trouvée. Le refuge des Fonts est accessible l'hiver à ski ; son ouverture n'est pas vérifiée.

**Chapelle-des-Bois et communes voisines**
- **Documents d'urbanisme :** le GPU ne liste aucun document pour 25121 (Chapelle-des-Bois), 25142 (Chaux-Neuve), 25413 (Mouthe) ou 39047 (Bellefontaine), alors que ces communes ne sont pas au règlement national d'urbanisme.
- **Arrêtés municipaux sur le camping ou le bivouac :** aucun trouvé.
- **Espace Nordique Jurassien (ENJ) :** sur les terrains privés traversés par les pistes, « rester sur la piste, bivouac et feu interdits » ([dépliant ENJ](https://www.montagnes-du-jura.fr/uploads/2022/12/enj_respect_depliant_2110_web.pdf)).
- **Aires de bivouac officielles :** aucune trouvée côté français.

## 4. Côté suisse (Risoud / Vallée de Joux, Vaud)

| Point | Règle | Source |
|---|---|---|
| Accès | Code civil suisse art. 699 : « Chacun a libre accès aux forêts et pâturages d'autrui ». LFo art. 14 : les cantons limitent l'accès si la protection de la faune l'exige. | [CC](https://fedlex.data.admin.ch/filestore/fedlex.data.admin.ch/eli/cc/24/233_245_233/20270101/fr/html/fedlex-data-admin-ch-eli-cc-24-233_245_233-20270101-fr-html-1.html), [LFo](https://fedlex.data.admin.ch/filestore/fedlex.data.admin.ch/eli/cc/1992/2521_2521_2521/20250801/fr/html/fedlex-data-admin-ch-eli-cc-1992-2521_2521_2521-20250801-fr-html-3.html) |
| Camping (loi vaudoise LCCR art. 27) | Le camping occasionnel « n'est permis qu'avec l'assentiment du propriétaire du fonds ». Au-delà de 4 jours, autorisation communale. Les dispositions communales sont réservées. | [LCCR 935.61](https://www.lexfind.ch/tolv/209386/fr) |
| Commune du Chenit | Affirme que le camping sauvage et le bivouac « n'[sont] pas autorisé[s] », avec trois tolérances : une nuit seul au-dessus de la limite des forêts, terrain privé avec accord, urgence. Base légale non citée. | [Chenit](https://www.communeduchenit.ch/vie-quotidienne/informations-pratiques/camping/) |
| Feu (Vaud) | Loi forestière vaudoise art. 33 : interdit « en forêt et à moins de dix mètres des lisières », sauf aux emplacements aménagés. | [vd.ch](https://www.vd.ch/themes/environnement/forets/incendies-de-foret/) |
| District franc fédéral Le Noirmont n° 30 | Ordonnance sur les districts francs, art. 5 e) : « il est interdit de camper librement » ; art. 5 g) : ski hors pistes et itinéraires balisés interdit. Géodonnées : règle R170, toute l'année. Il touche le bord sud de la zone de recherche. | [ODF](https://fedlex.data.admin.ch/filestore/fedlex.data.admin.ch/eli/cc/1991/2304_2304_2304/20250201/fr/html/fedlex-data-admin-ch-eli-cc-1991-2304_2304_2304-20250201-fr-html-4.html) |
| Zones de tranquillité | **Aucune** dans la zone ni dans la Vallée de Joux (identify geo.admin vide). Vaud : « premières zones… uniquement dans les Alpes ». | [vd.ch](https://www.vd.ch/environnement/biodiversite-et-paysage/sites-de-protection-de-la-faune-sauvage-et-corridors-a-faune) |
| Aires officielles | Mézery (Risoud) et L'Orient : de juin à octobre seulement, 3 nuits maximum. Rien en hiver. | [myvalleedejoux](https://www.myvalleedejoux.ch/en/Z14908/camping) |
| Autres couches dans la zone | Inventaire fédéral des paysages (IFP) « Vallée de Joux et Haut-Jura vaudois », site marécageux Vallée de Joux, Parc Jura vaudois, réseau Émeraude. Aucune règle de bivouac rattachée dans les données. | identify geo.admin |

Un skieur français qui passe en Suisse est soumis au droit suisse. Il lui faut l'accord du propriétaire (souvent une commune ou une corporation), et il doit éviter le Noirmont et les lisières pour le feu.

## 5. Données ouvertes

| Jeu | Accès | Format | Licence | Mise à jour | Test |
|---|---|---|---|---|---|
| Espaces protégés INPN : APPB, réserves naturelles, PNR, parcs nationaux, sites classés, Natura 2000 (SIC/ZPS), réserves biologiques… | WFS `https://data.geopf.fr/wfs/ows`, couches `patrinat_apb:apb`, `patrinat_rnn:rnn`, `patrinat_rnr:rnr`, `patrinat_pnr:pnr`, `patrinat_pn:parc_national`, `patrinat_sc:sc`, `patrinat_sic:sic`, `patrinat_zps:zps`, `patrinat_rb:rb` | GeoJSON, renvoyé **en EPSG:3857 par défaut** ; BBOX en ordre lat,lon avec `urn:ogc:def:crs:EPSG::4326` | « other-open » selon data.gouv | semestrielle, ressources du 08/10/2026 | **OK.** Les shapefiles inpn.mnhn.fr/docs/Shape/*.zip répondent **403** (défi Cloudflare). data.gouv signale une cyberattaque au MNHN. |
| Fiche data.gouv | [INPN Espaces protégés](https://www.data.gouv.fr/datasets/inpn-donnees-du-programme-espaces-proteges) | — | — | — | — |
| Servitudes GPU : AC1 (monuments historiques), AC2 (sites inscrits et classés), AC4 (SPR), AS1 (captages), A7 (forêts de protection) | WFS `wfs_sup:assiette_sup_s` | GeoJSON EPSG:4326 | non vérifiée | variable | **OK.** C'est la seule source qui donne les **sites inscrits** et les SPR, c'est-à-dire R111-33 directement. **Aucune AS1** n'est remontée dans les deux zones, ce qui ne prouve pas l'absence de captage. |
| Biodiv'sports | `https://biodiv-sports.fr/api/v2/sensitivearea/?in_bbox=…&format=geojson` | GeoJSON | non vérifiée | — | **OK** (10 et 6 zones). **Aucune catégorie « bivouac »** parmi les 7 pratiques, `rules` est vide, et « bivouac » n'apparaît dans aucune description, même pour l'APPB Paradis. Les APPB Haut-Jura et Haut-Doubs ne sont pas des zones distinctes. |
| Suisse : districts francs / sites de protection de la faune | STAC `data.geo.admin.ch` : collections `ch.bafu.wrz-jagdbanngebiete_select` (mise à jour le 12/11/2026) et `ch.bafu.bundesinventare-jagdbanngebiete` | FileGDB / SHP en EPSG:2056, plus API identify | opendata.swiss « terms_by » (attribution) | — | Archive téléchargée (1,1 Mo, documentation seulement) ; identify OK |
| Suisse : zones de tranquillité | Couche `ch.bafu.wrz-wildruhezonen_portal` via l'API identify, attributs `schutzzeit` et `best_fr` | JSON | — | — | La couche répond (134 objets aux Grisons), mais je ne l'ai pas trouvée dans STAC |

Les données téléchargées sont dans `research/raw/dl/`, les PDF dans `.../scratchpad/pdf/`, le script d'intersection est `research/raw/inter.py`.

**Conséquence pour l'app :** les règles de bivouac (périodes, zones cœur, parcelles B771/B775) ne figurent dans aucun flux. Il faudra les saisir à la main à partir des arrêtés.

## 6. Synthèse par site

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Bivouac autorisé ? | Oui par défaut (R111-32), aucune interdiction communale trouvée | Rarement en hiver |
| Où l'éviter | SPR du village, abords des monuments historiques, Casse Déserte (Arvieux), Clarée ; côté Arvieux, à moins d'une heure de marche des hameaux | Site inscrit (fond de vallée, lac des Mortes) ; APPB Haut-Jura et Haut-Doubs du 15/12 au 30/06 ; zones cœur Risol et Verdet toute l'année ; Forêt du Paradis toute l'année ; terrains privés traversés par les pistes ENJ ; Noirmont (CH) |
| Horaires et durée | Pas de règle à Cervières. Arvieux : 18 h–9 h, une nuit par secteur. | Pas de règle trouvée. Accès de nuit interdit dans les zones cœur Haut-Doubs. |
| Feu | Interdit, **réchauds compris** (commune) ; période rouge en été | Interdit à moins de 200 m des forêts (L131-1). Interdit dans les APPB. Vaud : interdit à moins de 10 m des lisières. |
| Accord du propriétaire | Requis (R111-32) | Requis (R111-32 ; LCCR art. 27 côté Vaud) |

## Non vérifié

- Texte signé de l'APPB Haut-Doubs (31/12/2025) et texte signé de l'APPB Haut-Jura du 27/05/2019. Je cite le projet, la synthèse et la plaquette.
- Arrêté de Cervières sur le feu et les réchauds (08/07/2022) : connu seulement par la page du PNR Queyras.
- Arrêtés municipaux de Chapelle-des-Bois, Bellefontaine, Chaux-Neuve et Mouthe : aucun trouvé. Leurs documents d'urbanisme sont absents du GPU.
- Règles de l'ONF pour la forêt domaniale du Risoux et pour la réserve biologique du Bois des Ayes.
- Règlement de police du Chenit ; existence de réserves cantonales vaudoises dans le Risoud (je n'ai pas consulté geo.vd.ch).
- Licences exactes des flux Géoplateforme, GPU et Biodiv'sports.
- Captages d'eau (AS1) : non publiés dans le GPU pour ces zones.
- Statut juridique du bivouac au regard de R111-33 : aucune jurisprudence trouvée.
- Natura 2000 : je n'ai pas lu de texte primaire confirmant qu'aucune interdiction ne s'applique.
- Règles propres aux APPB Corniches calcaires et Écrevisse.
- La page de téléchargement temporaire PatriNat n'a pas répondu (délai dépassé).
