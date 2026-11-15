# skimap-pisteplan

Plan des pistes officiel transcrit sur nos cartes. Ticket : P22 (#121).

On ne redistribue pas le plan, et on ne l'affiche pas en image sur la carte. Il sert seulement d'entrée locale au pipeline. Ses attributs (nom, couleur, départs, longueurs, boucle) sont reportés sur **nos** géométries OSM. Le style de la carte les dessine ensuite comme nos propres pistes.

- **Sans fonctionnalité** (téléphone) : `transcript` lit la transcription (`read_pistes`, `read_landmarks`, propriétés typées). Dépendances : serde, serde_json, skimap-common, thiserror. Compile pour `aarch64-linux-android`.
- **Fonctionnalité `pipeline`** : `build::build(site, &layout, &downloader, personal_sources)`.

## Plans (usage personnel, `personal_sources`)

| Site | Plan | URL, date | Droits affichés |
|---|---|---|---|
| Cervières | « Plan des pistes - Cervières » (dépliant « L'Izoard, hiver ») | [nordicfrance.fr/…/Plan-des-pistes-Cervieres.pdf](https://www.nordicfrance.fr/content/uploads/2018/12/Plan-des-pistes-Cervieres.pdf), lié par nordicalpesdusud.com/domaines/cervieres (saison 2025-2026) ; PDF Canva du 27/03/2024, image 1280 × 1255 px sans couche texte | aucune licence ; « Conception graphique : Emilie Chenier » |
| Chapelle-des-Bois | « Plan des pistes ski de fond, Lacs et Montagnes du Haut-Doubs (Sud) » | `ENJ_Plans_CCLMHD_VALdeMOUTHE_2511_WEB_ski.pdf`, lié par espacenordiquejurassien.com/haut-doubs.html ; « Décembre 2025 », PDF InDesign du 17/02/2026, 1/50 000, couche texte | aucune licence ; « Cartographie Latitude Cartagène », « © les contributeurs d'OpenStreetMap » |

Aucun plan n'est choisi à la main, aucune fiche ni aucun réglage n'est propre à un site. `build(site)` (`register_site`) prend dans le catalogue du collecteur les plans rattachés à un domaine du recensement qui touche l'emprise du site, les lit tous avec la fiche automatique et garde celui dont l'emprise dessinée touche le site avec le plus de pistes transcrites par l'appariement exact (la liste des plans essayés est dans `stats.plans_considered`). Cervières : 3 plans essayés, nf-14976 retenu ; Chapelle-des-Bois : 12 essayés, 9 calés, le plan ENJ du Val de Mouthe retenu (28 pistes exactes contre 19 au plan suivant). Les rendus restent dans `data/cache/pisteplan/`.

**Collecteur** (`collect`, exemple `collect_plans`) : médiathèque WordPress de Nordic France (recherches « plan » et « piste », station de rattachement → domaine du recensement) et pages des massifs de l'ENJ. Les fichiers sont choisis sur leur titre (raquettes, piétons, alpin, navettes… écartés). Sources propres, une requête toutes les 5 s. Au 15/11/2026 : 298 candidats, 257 plans distincts (41 doublons, 0 erreur), dont 114 PDF avec couche texte, 105 PDF-image, 15 PDF vectoriels sans texte, 22 images. 159 sont rattachés à un domaine (station, sites ou nom de la page) ; 99 des 219 domaines ont au moins un plan. Dates de création de 2009 à 2026, surtout 2018-2026. Sortie : `data/out/france/plans/catalogue.json` (usage personnel).

## Chaîne

1. Entrée : `pdftoppm` (rendu PPM) et `pdftotext -bbox-layout` (couche texte), outils poppler appelés en sous-processus. Tout le reste est en Rust.
2. Encres de la légende : segmentation CIELAB, fermeture, trous comblés (flèches), squelette Zhang-Suen, graphe du dessin.
3. Étiquettes : couche texte, ou OCR `ocrs` (pur Rust) par tuiles de 800 px. Sur un plan-image, le texte est d'abord rehaussé par un chapeau haut-de-forme. La légende donne le nom, l'encre (tiret échantillonné ou mot « verte »…), la longueur et le départ.
4. OSM : pistes de l'extrait OpenSnowMap sur l'emprise du plan, repères nommés par Overpass (lieux, sommets, cols, refuges, lacs).
5. Calage. Un RANSAC exhaustif apparie étiquettes et repères (similitude ≤ 45°, tolérance 15 pt), puis une ICP non rigide ajuste une spline plaque mince. Elle apparie les encres de ski aux voies nordiques de même couleur, avec un rayon qui décroît de 600 à 70 m. Points d'appui : jonctions du dessin et jonctions OSM nordiques, plus proches voisins mutuels, sans ambiguïté.
6. Appariement **exact**. Il faut d'abord un nom certain : clé identique sur une couche texte, ou une lecture OCR unique (similarité ≥ 0,6 et écart ≥ 0,15). Les homonymes non reliés sont départagés par la proximité de l'encadré. Il faut ensuite que l'encre de la piste soit dessinée le long de la géométrie OSM, dans le même sens (≤ 30°), sur au moins 50 %. Cette part doit aussi dépasser celle des 16 copies décalées (400 et 800 m) : test de permutation, p ≤ 1/17. Le rayon (3, 5, 8 ou 12 pt) est choisi par validation sur les candidats. Une piste qui échoue n'est pas transcrite : elle est listée avec la raison.

**Fiche automatique** (`autospec`, `register_auto`), la seule : la fiche de lecture (`PlanSpec`) est déduite du plan seul. Résolution et source des étiquettes viennent du PDF (`pdfinfo`, `pdfimages`). Les encres sont lues sur la légende (tiret de chaque piste, mot de couleur), puis ajustées sur les lignes de la carte (pixels contrastés, même teinte). Une couleur tracée sans entrée de légende devient une encre non ski. Les lignes de légende d'un tableau (nom, puis longueur dans une colonne à part) sont recollées comme sur une lecture OCR ; sans tiret à côté du nom, la marque de la ligne est cherchée plus à gauche sur la même rangée (pastille numérotée, pictogramme ; jusqu'au texte précédent, 200 pt au plus). Une qualification entre parenthèses en fin de nom (« Les Coulets (côté Nord) », « (Multi-activités) ») est retirée quand aucune piste OSM ne porte le nom complet.

**Encres lues sur le dessin** (`autospec::drawing_inks`) : quand la légende ne donne pas d'encre de difficulté à deux lignes au moins (pictogrammes, numéros, légende générique sans longueur), les couleurs des tracés eux-mêmes deviennent les encres. Pixels contrastés (≥ 20 avec leur voisinage) de la zone de carte, nommés vert, bleu ou rouge par leur teinte CIELAB ; regroupés par nom, puis par couleur (ΔE 20 autour du casier de 4 bits le plus fréquent, un groupe pèse au moins 15 % du nom et 200 pixels) ; référence d'un groupe = médiane de son cinquième le plus contrasté (centre des traits). Un groupe à moins de ΔE 16 d'une couleur de fond de la carte est la texture du fond, pas une encre. Une encre est gardée si elle trace au moins 200 pt de traits fins (composantes de 20 pt de squelette, 4 pt de large au plus). Chaque encre gardée est une piste de ski de sa couleur (échelle française vert, bleu, rouge) ; le noir n'est pas pris sur le dessin (routes, courbes, texte), les autres couleurs restent non ski. Source de chaque encre : `legend`, `drawing` ou `unlisted` (`AutoReport.inks[].source`). L'épaisseur des traits donne les paramètres de ligne. Les panneaux et encadrés sont des aplats rectangulaires. La zone de carte entoure le réseau dessiné. L'emprise est l'étendue du plan une fois placé par le RANSAC, plus 3 km.

**Plans difficiles (X1, #135)** :

- Images PNG et JPEG : décodées (`png`, `zune-jpeg`), lues comme une page à 72 ppi puis rééchantillonnées comme un PDF-image (2 ×, OCR).
- Plans sans domaine du recensement (collecteur) : localisés par les lieux qu'ils nomment (`collect::locate` : nom, commune, stations Nordic France de chaque domaine, en mots entiers dans le texte, le titre ou le nom du fichier ; le domaine le plus cité et ceux à moins de 25 km). Un mauvais domaine ne se cale pas : rien n'est transcrit.
- Pages à plusieurs cartes : chaque réseau dessiné qui pèse au moins 25 % du principal est une carte (`autospec::networks`) ; si la carte principale ne se cale pas, les autres sont essayées tour à tour.
- Panoramas (`perspective`) : quand aucun calage à plat ne tient, une caméra projective (DLT 3 × 4 : position, cap, inclinaison, focale) est cherchée par RANSAC sur six paires étiquette ↔ repère, hauteurs lues sur le MNT LiDAR HD (`skimap-terrain`, 5 m moyenné à 25 m, domaines + 3 km). Elle doit réunir 8 étiquettes à 30 pt, se tenir au-dessus du relief, tous les repères devant elle. Un point de la page est porté sur la carte par son rayon (première rencontre avec le relief), point de départ de l'ICP non rigide.

## Apprentissage (étiquetage automatique, modèle)

1. **Étiquetage** (`learn::pairs`, exemple `train_pairs`) : chaque PDF du catalogue rattaché à un domaine est lu avec une fiche automatique et calé ; les repères viennent de l'IGN (BD TOPO, toponymie) quand Overpass est injoignable. Pour chaque ligne de légende, les candidats sont les pistes OSM (relations d'un même nom, reliées) où l'encre est dessinée sur au moins 10 % (à 12 pt). Une paire vaut 1 si l'appariement exact a porté la ligne sur cette piste, 0 si l'exact l'a portée sur une autre ; les lignes sans appariement exact ne sont pas étiquetées. Sortie : `data/out/france/plan_training/plans/<plan>.json` (usage personnel). Au 15/11/2026 : 158 PDF tentés, 46 calés (112 échecs : 64 sans ensemble cohérent étiquettes ↔ repères, 48 sans paires d'ICP ; surtout des panoramas et des plans-images), 18 avec des appariements exacts ; 181 paires positives, 6 419 négatives, 11 532 non étiquetées. Plans calés et positifs : massifs du Jura (ENJ, Les Rousses, Retord, Pontarlier) et 3 plans-images alpins. 34 min pour les 156 premiers plans (3 en parallèle, machine chargée) ; l'OCR des plans-images domine.
2. **Descripteurs** (27, `learn::FEATURES`) : part dessinée à 3, 5, 8, 12 pt et la même pour les 16 copies décalées, autres encres, couleur OSM, longueur / longueur imprimée, distance à l'encadré du départ, étiquettes de la carte près de la géométrie, boucle, rang parmi les candidats, qualité du calage, similarité du nom de légende avec le nom OSM.
3. **Modèle** : arbres de décision renforcés (`gbdt`, Rust pur, 200 arbres de profondeur 3). Évaluation par domaine (`evaluate`) : plans regroupés par domaine dessiné (au plus 5 plis), jamais un domaine en entraînement et en test. Les plans des deux sites et tout plan qui dessine un de leurs domaines sont exclus de l'entraînement. Décision par ligne : meilleur candidat si p ≥ seuil et aucun autre candidat au seuil. Seuil : plus petit seuil à partir duquel la précision mesurée reste ≥ 99 %, et pas moins de 0,9. Oracle Python (scikit-learn, hors dépôt) sur les mêmes plis. Résultat : 10 plans d'entraînement, 3 groupes de domaines seulement (les plans du Jura se recouvrent), 1 973 paires dont 75 positives ; AUC hors pli 0,9995 (oracle 0,987) ; au seuil 0,9 : 61 lignes décidées, 61 justes (précision 100 %, borne basse 95 % 0,94), rappel 0,81. Entraînement : 0,3 s.
4. **Transcription** : `transcribe_and_write` applique `data/out/france/plan_training/model.json` s'il existe aux lignes que l'exact a laissées : `method` = `ml`, `score` = probabilité, `osm_status` = `name_missing` si la piste OSM porte un autre nom. Les étiquettes exigent le nom : la précision mesurée vaut pour les lignes que l'exact tranche ; sur les autres, elle est vérifiée à la main sur les deux sites.
5. **Évaluation par massif (X3, #137)** (`evaluate::by_massif`, `train_model`) : massif d'un plan = massif le plus fréquent des départements de ses domaines dessinés (table nationale : Jura 01-25-39, Alpes du Nord 26-38-73-74, Alpes du Sud 04-05-06, Vosges, Massif central, Pyrénées, Corse). Tous les plans étiquetés, sites d'essai compris, sont évalués ; chaque massif est tenu à l'écart à son tour (modèle entraîné sur les autres massifs seulement), et la validation croisée par groupe de domaines est aussi restreinte au massif. Le même calcul sans les descripteurs de nom (`name_similarity`, `map_label_similarity`) mesure le cas d'une piste dont le nom OSM n'est pas celui du plan ; les étiquettes restent celles de l'exact. `TRAIN_PLANS_DIR=x2_step1` (étiquetage après les encres lues sur le dessin, 27 plans étiquetés, 242 paires positives), seuil 0,9 :

   | Massif | plans | paires (+) | tenu à l'écart : décidées / justes, précision (borne 95 %), rappel | sans nom, tenu à l'écart : précision, rappel |
   |---|---|---|---|---|
   | Jura | 21 | 6 652 (224) | 211 / 211, 1,000 (0,982), 0,942 | 0,300 (21 / 70), 0,094 |
   | Alpes du Sud | 4 | 94 (14) | 12 / 12, 1,000 (0,757), 0,857 | 1,000 (6 / 6), 0,429 |
   | Alpes du Nord | 2 | 57 (4) | 4 / 4, 1,000 (0,510), 1,000 | 1,000 (1 / 1), 0,250 |

   Trois massifs seulement ont des étiquettes : des plans sont calés dans les Vosges (3), le Massif central (4) et les Pyrénées (1), sans appariement exact (pas de ligne de légende lisible avec longueur), donc sans paire étiquetée. Sans le nom, le modèle appris sur les Alpes ne transfère pas au Jura (précision 0,30) : le nom porte la décision.

## Sorties (`data/out/<site>/plan_pistes/`)

| Fichier | Contenu |
|---|---|
| `plan_pistes.geojson` | une `MultiLineString` OSM par piste du plan appariée : `name`, `name_read`, `colour`, `ink`, `kind`, `lengths_km_plan` (pas pour un plan lu par OCR), `departures`, `length_m`, `loop`, `method` (`exact` ou `ml`), `score`, `geometry_source`, `osm_status` (`match`, `colour_differs`, `colour_missing`, `name_missing`), `osm_name`, `osm_colour`, `osm_relation_ids`, `osm_way_ids` |
| `plan_ways.geojson` | voies OSM des pistes transcrites qui portent des flèches sur le plan : `direction` (`forward`/`backward` par rapport à l'ordre de la voie, `both`), nombre de flèches, `osm_oneway`, `oneway_status` (`match`, `missing_in_osm`, `opposite`…) |
| `plan_landmarks.geojson` | repères du plan appariés à OSM (lieux, sommets, cols, refuges, départs) |
| `plan_pistes.meta.json` | sources (plan : `personal_use_only`), paramètres, `stats` : décisions, raisons des refus, validation, RANSAC, ICP, points d'appui, validation croisée, OCR |

```sh
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example plan_pistes -- cervieres chapelle-des-bois
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example ocr_gold   # OCR Cervières
cargo test -p skimap-pisteplan --features pipeline
cargo test -p skimap-pisteplan --test real_outputs -- --ignored   # lecteur sur les sorties réelles
# diagnostics locaux (images du plan, jamais publiées) : examples register, diag_plan
# collect_plans, train_pairs [jobs], train_model [rows.csv] : collecte, étiquetage, modèle (data/out/france/)
```

Démo : `demo/compare.html?site=…&view=osm|plan` (style et couleurs du visualiseur, sans image du plan), à servir depuis la racine par `skimap serve`.

## Résultats (15/11/2026)

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Pistes de la légende (lignes) | 8 lues sur 9 (OCR) | 56 (59 lignes) |
| Avec une piste OSM du même nom | 6 | 39 |
| Transcrites (exact) | 3 (Gibera, Pinatelle, Chemin du Sioux) | 30 (54 % du plan, 77 % des candidats) |
| Fiche automatique seule (15/11/2026, sans fiche à la main) : exact + modèle | 3 + 0 (Gibera, Pinatelle, Chemin du Sioux à 50 %) | 28 + 6 = 34 |
| Même fiche, encres du dessin, marques de rangée, qualificatifs retirés (15/11/2026) : exact + modèle | 3 + 0 (inchangé : la légende a ses tirets) | 29 + 5 = 34 (Combe des Cives (Multi-activités) passe du modèle à l'exact) |
| Transcrites, exact + modèle (sorties) | 3 + 0 | **30 + 6 = 36** (64 %) : Le Pré d'Haut (OSM « Les Prés d'Haut »), Combe des Cives, Le Griffon (« Le Griffond »), Élodie Bourgois Pin (« Elodie Bourgeois Pin »), La Passerelle, Le Petit Glacier ; 6/6 justes à l'écran |
| Couleur du plan = couleur OSM | 3 / 3 | 30 / 30 |
| Repères appariés | 14 | 16 |
| Voies avec flèches (sens) : même sens qu'OSM / sens absent d'OSM / opposé | sans flèches | 29 / 19 / 0 (+ 1 dans les deux sens) |
| Lecture OCR | 45 % des 62 étiquettes relevées à la main (16 % exactes) | 52 % des 566 mots de la couche texte (48 % exacts) |
| Appariement RANSAC | 14 étiquettes sur 27 candidates, 28,5 m/pt | 12 sur 53, 17,4 m/pt |
| ICP : écart médian ligne ↔ OSM | 6 m (73 paires) | 7 m (1 152 paires) |
| Points d'appui (jonctions) | 6 | 39 |
| Validation croisée, laisser-un-dehors (médiane / p90) | spline λ 0,01 : 78 / 421 m ; affine 99 / 282 m | spline λ 0,1 : 44 / 86 m ; affine 74 / 137 m ; homographie 78 / 144 m |
| Déformation dense, jonction tenue à l'écart | 62 / 88 m | 41 / 70 m |

Le plan ENJ est une carte au 1/50 000 dessinée sur OSM : même là, la spline fait mieux que les modèles globaux (perspective comprise). Le plan de Cervières est un dessin stylisé : six jonctions seulement, la mesure y est fragile.

Vérification à la main contre le plan (encadrés et légende relus à l'écran) :

- Chapelle-des-Bois, 30 pistes transcrites : noms 30/30, couleurs 30/30, départs et longueurs 29/30. « Piste débutant » de Source du Doubs (1,0 km) est rattachée à la relation de Chez Liadet : OSM n'a qu'une relation de ce nom.
- Cervières, 3 pistes : noms 3/3 (Le Gibera, La Pinatelle, Le Chemin du Sioux), couleurs 3/3, secteurs 3/3.

La relecture a aussi trouvé deux erreurs, corrigées depuis : « Combe Verte » (bleue) était lue verte à cause du mot « Verte », et « Le Gy » était coupée par la couche texte.

## Limites

- Cervières : image floue de 1 280 px. Les lignes y sont fines, la piste bleue a presque la couleur du fond turquoise, et l'OCR ne lit pas « Savare » (ambigu), « Izoard » ni les deux liaisons. Les longueurs lues par OCR sont fausses (« 75km » pour 7,5 km), elles ne sont pas transcrites.
- 17 pistes de Chapelle-des-Bois n'ont aucune relation OSM du même nom ; le modèle en rattache 4 à une relation d'une autre graphie. Restent 13 (les liaisons, Le Gy, La Lanterne…), listées, pas devinées : c'est l'entrée de G1, sans tracé estimé à ce jour. Combe des Cives : la relation OSM (7,4 km) déborde le trait du plan (11 km imprimés).
- Sens : flèches lues à Chapelle-des-Bois seulement. Le plan de Cervières n'en dessine pas. Une flèche est un trou de l'encre (triangle blanc cerné) ; son sens est le signe de l'asymétrie de ses pixels le long de la ligne. Dans un extrait relu à la main, 10 flèches sur 12 sont trouvées. Le sens est donné par voie (`plan_ways.geojson`), pas par piste.
- Les PDF n'ont ni `ETag` ni `Last-Modified` : un nouveau plan n'est vu qu'à l'expiration du cache (30 jours).
- Le test exact est sensible au calage : à Chapelle-des-Bois, de petits écarts de fiche (zone de carte au point près, masques, encres) font varier le nombre de pistes transcrites de 25 à 32. Les pistes qui basculent sont celles dont la part dessinée est proche de 50 % ou des copies décalées.
- Encres lues sur le dessin (légendes sans tiret coloré) : mesure sur les 256 plans du catalogue, même machine, mêmes caches, avant / après (`train_pairs`, dossiers `x2_base` et `x2_step1`) : plans calés 63 → 98, plans avec des appariements exacts 22 → 27, appariements exacts 189 → 217, paires positives 212 → 242 ; échecs d'ICP 57 → 13. Aucun plan calé avant ne l'est plus ; un seul perd un appariement (Grand Pontarlier 3 → 2, instabilité du test exact, X4). Par format (calés avant → après) : PDF texte 31 → 47, PDF-image 25 → 38, images 7 → 12, PDF vectoriels 0 → 1. Un premier panorama est calé (nf-7199, caméra sur 8 étiquettes). Limite : 17 des 35 nouveaux calages ne reposent que sur 2 étiquettes du RANSAC (comme avant, l'ICP était le seul garde-fou) ; ils ne donnent aucun appariement exact sauf nf-19186 (1). Les plans nouvellement calés sans exact n'ont pas de ligne de légende lisible (pas de longueur imprimée) : ils servent au calage, pas encore à l'étiquetage.
