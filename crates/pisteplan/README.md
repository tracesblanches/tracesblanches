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

Téléchargés par le téléchargeur commun (sources `nordicfrance`, `enj`, cache 30 jours). Les rendus restent dans `data/cache/pisteplan/`.

**Collecteur** (`collect`, exemple `collect_plans`) : médiathèque WordPress de Nordic France (recherches « plan » et « piste », station de rattachement → domaine du recensement) et pages des massifs de l'ENJ. Les fichiers sont choisis sur leur titre (raquettes, piétons, alpin, navettes… écartés). Sources propres, une requête toutes les 5 s. Au 15/11/2026 : 298 candidats, 257 plans distincts (41 doublons, 0 erreur), dont 114 PDF avec couche texte, 105 PDF-image, 15 PDF vectoriels sans texte, 22 images. 159 sont rattachés à un domaine (station, sites ou nom de la page) ; 99 des 219 domaines ont au moins un plan. Dates de création de 2009 à 2026, surtout 2018-2026. Sortie : `data/out/france/plans/catalogue.json` (usage personnel).

## Chaîne

1. Entrée : `pdftoppm` (rendu PPM) et `pdftotext -bbox-layout` (couche texte), outils poppler appelés en sous-processus. Tout le reste est en Rust.
2. Encres de la légende : segmentation CIELAB, fermeture, trous comblés (flèches), squelette Zhang-Suen, graphe du dessin.
3. Étiquettes : couche texte, ou OCR `ocrs` (pur Rust) par tuiles de 800 px. Sur un plan-image, le texte est d'abord rehaussé par un chapeau haut-de-forme. La légende donne le nom, l'encre (tiret échantillonné ou mot « verte »…), la longueur et le départ.
4. OSM : pistes de l'extrait OpenSnowMap sur l'emprise du plan, repères nommés par Overpass (lieux, sommets, cols, refuges, lacs).
5. Calage. Un RANSAC exhaustif apparie étiquettes et repères (similitude ≤ 45°, tolérance 15 pt), puis une ICP non rigide ajuste une spline plaque mince. Elle apparie les encres de ski aux voies nordiques de même couleur, avec un rayon qui décroît de 600 à 70 m. Points d'appui : jonctions du dessin et jonctions OSM nordiques, plus proches voisins mutuels, sans ambiguïté.
6. Appariement **exact**. Il faut d'abord un nom certain : clé identique sur une couche texte, ou une lecture OCR unique (similarité ≥ 0,6 et écart ≥ 0,15). Les homonymes non reliés sont départagés par la proximité de l'encadré. Il faut ensuite que l'encre de la piste soit dessinée le long de la géométrie OSM, dans le même sens (≤ 30°), sur au moins 50 %. Cette part doit aussi dépasser celle des 16 copies décalées (400 et 800 m) : test de permutation, p ≤ 1/17. Le rayon (3, 5, 8 ou 12 pt) est choisi par validation sur les candidats. Une piste qui échoue n'est pas transcrite : elle est listée avec la raison.

**Fiche automatique** (`autospec`, `register_auto`) : la fiche de lecture (`PlanSpec`) est déduite du plan seul. Résolution et source des étiquettes viennent du PDF (`pdfinfo`, `pdfimages`). Les encres sont lues sur la légende (tiret de chaque piste, mot de couleur), puis ajustées sur les lignes de la carte (pixels contrastés, même teinte). Une couleur tracée sans entrée de légende devient une encre non ski. L'épaisseur des traits donne les paramètres de ligne. Les panneaux et encadrés sont des aplats rectangulaires. La zone de carte entoure le réseau dessiné. L'emprise est l'étendue du plan une fois placé par le RANSAC, plus 3 km.

## Sorties (`data/out/<site>/plan_pistes/`)

| Fichier | Contenu |
|---|---|
| `plan_pistes.geojson` | une `MultiLineString` OSM par piste du plan appariée : `name`, `name_read`, `colour`, `ink`, `kind`, `lengths_km_plan` (pas pour un plan lu par OCR), `departures`, `length_m`, `loop`, `method` (`exact`), `score`, `geometry_source`, `osm_status` (`match`, `colour_differs`, `colour_missing`, `name_missing`), `osm_name`, `osm_colour`, `osm_relation_ids`, `osm_way_ids` |
| `plan_ways.geojson` | voies OSM des pistes transcrites qui portent des flèches sur le plan : `direction` (`forward`/`backward` par rapport à l'ordre de la voie, `both`), nombre de flèches, `osm_oneway`, `oneway_status` (`match`, `missing_in_osm`, `opposite`…) |
| `plan_landmarks.geojson` | repères du plan appariés à OSM (lieux, sommets, cols, refuges, départs) |
| `plan_pistes.meta.json` | sources (plan : `personal_use_only`), paramètres, `stats` : décisions, raisons des refus, validation, RANSAC, ICP, points d'appui, validation croisée, OCR |

```sh
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example plan_pistes
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example ocr_gold   # OCR Cervières
cargo test -p skimap-pisteplan --features pipeline
cargo test -p skimap-pisteplan --test real_outputs -- --ignored   # lecteur sur les sorties réelles
# diagnostics locaux (images du plan, jamais publiées) : examples inspect, register, ocr_dump
SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example auto_compare  # fiche auto / à la main
```

Démo : `demo/compare.html?site=…&view=osm|plan` (style et couleurs du visualiseur, sans image du plan), à servir depuis la racine par `skimap serve`.

## Résultats (15/11/2026)

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Pistes de la légende (lignes) | 8 lues sur 9 (OCR) | 56 (59 lignes) |
| Avec une piste OSM du même nom | 6 | 39 |
| Transcrites (exact) | 3 (Gibera, Pinatelle, Chemin du Sioux) | 30 (54 % du plan, 77 % des candidats) |
| Fiche automatique : transcrites (exact) | 2 sur les 3 (Chemin du Sioux : 48 % < 50 %) | 32 : les 30 + La Chaux Mourant, Le Petit Glacier (justes, vérifiées à l'écran) |
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
- 17 pistes de Chapelle-des-Bois n'ont aucune relation OSM du même nom (Le Pré d'Haut, Combe des Cives, les liaisons, Le Gy, La Lanterne…). Elles sont listées, pas devinées : c'est l'entrée de G1.
- Sens : flèches lues à Chapelle-des-Bois seulement. Le plan de Cervières n'en dessine pas. Une flèche est un trou de l'encre (triangle blanc cerné) ; son sens est le signe de l'asymétrie de ses pixels le long de la ligne. Dans un extrait relu à la main, 10 flèches sur 12 sont trouvées. Le sens est donné par voie (`plan_ways.geojson`), pas par piste.
- Les PDF n'ont ni `ETag` ni `Last-Modified` : un nouveau plan n'est vu qu'à l'expiration du cache (30 jours).
- Le test exact est sensible au calage : à Chapelle-des-Bois, de petits écarts de fiche (zone de carte au point près, masques, encres) font varier le nombre de pistes transcrites de 25 à 32. Les pistes qui basculent sont celles dont la part dessinée est proche de 50 % ou des copies décalées.
- Fiche automatique : les plans-dessins en perspective (panoramas des stations alpines), les pages à plusieurs cartes et les légendes sans tiret coloré ne sont pas lus ; seuls les plans calés par leurs étiquettes servent à l'apprentissage.
