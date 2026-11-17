# skimap-zones

Zones protégées et leurs périodes, règles de bivouac et d'accès. Tickets : P9 (#22), P10 (#23), G4 (#120).

## Deux usages

- **Sans fonctionnalité** : le schéma de la couche (`layer`) et des règles (`rules`), sans pile HTTP. `skimap-core` (C8, `zone_eval`) en dépend sur le téléphone.
- **Fonctionnalité `pipeline`** : les sources et `build(site, &DataLayout, &Downloader)`, qui écrit `data/out/<site>/zones/`.

```sh
cargo run --release -p skimap-zones --features pipeline --example build_zones            # les deux sites
cargo test -p skimap-zones --features pipeline --test real_sites -- --ignored              # contrôles sur les couches produites
```

## Sorties (`data/out/<site>/zones/`)

| Fichier | Contenu |
|---|---|
| `zones.geojson` | toutes les zones découpées à l'emprise du site, WGS84, coordonnées arrondies à 1e-6° |
| `rules.json` | les règles saisies à la main, copiées telles quelles |
| `zones.meta.json` | sources, licences, date de génération, validité, propriétés, nombre de zones par type, zones de chaque règle, surfaces |
| `build_report.json` | résumé de la construction |

Propriétés d'une zone : `id` (`inpn:FR3800988`, `gpu:<idass>`, `biodiv:1183`, `ch-jbg:980`, `country:fr`, `dept:25`, `canton:vd`, `commune:05027`, `commune:ch-5872`), `kind`, `name`, `source`, `url`, `months` (mois sensibles Biodiv'sports, sinon null), `practices`, `designated`, `area_ha` (surface géodésique dans l'emprise), `clipped`, `note`.

## Sources (P9)

| Source | Accès | Contenu gardé |
|---|---|---|
| INPN (PatriNat) | WFS `data.geopf.fr`, couches `patrinat_apb:apb`, `rnn`, `rnr`, `pnr`, `parc_national`, `sc`, `sic`, `zps`, `rb` ; sortie demandée en CRS:84 (EPSG:3857 par défaut, reprojeté s'il revient) | tout |
| GPU, servitudes | WFS `wfs_sup:assiette_sup_s` | AC1 (monuments et abords), AC2 (sites inscrits et classés), AC4 (SPR), AS1 (captages), A7 (forêts de protection) |
| Biodiv'sports | `api/v2/sensitivearea/?in_bbox=…&period=ignore` (sans `period`, seulement le mois courant) | toutes les zones, avec leurs mois et pratiques |
| geo.admin.ch | `identify` et `find` | districts francs fédéraux (parties à protection intégrale ou partielle), cantons, communes citées par les règles |
| geo.api.gouv.fr | contours des communes par département | France, départements, communes citées par les règles ; départements trouvés par une grille de 11 × 11 points |

Sites inscrits : une servitude AC2 qui recouvre à plus de 50 % un site classé INPN est ce site classé (elle est fusionnée dans la zone INPN) ; les autres sont des sites inscrits, sauf si leur nom dit « Site Classé ».

Géométries : les parties multiples qui se chevauchent (Biodiv'sports publie des multipolygones invalides) sont fusionnées avant découpage ; les surfaces sont calculées anneau par anneau, indépendamment du sens des anneaux.

## Règles (P10)

Fichier `rules/rules.json`, validé à la compilation des tests (`RuleSet::validate`). Chaque règle a :

- `topic` : `bivouac`, `leave_routes`, `fire`, `access` ;
- `effect` : `forbidden`, `tolerated`, `consent_required`, `avoid_recommended`, `allowed` ;
- `applies_to` : zones par identifiant ou par type (`kind`), éventuellement par pratique Biodiv'sports ;
- `period` : `all_year`, `yearly` (`from`/`to` en `MM-JJ`, passage d'année possible), `night` (du coucher au lever du soleil au lieu de référence), `zone_months` (mois sensibles de la zone) ;
- `in_force` : dates de vigueur du texte ;
- `stoves` (feu) : réchauds compris ou non, `null` si le texte ne dit rien ;
- `only_where` : la règle ne vise qu'une partie de la zone sans contour (200 m des bois, terrains privés traversés par les pistes) ;
- `exceptions` : sans contour (parcelles B 771 et B 775 de Foncine-le-Haut, renumérotées depuis 1986) ;
- `verification` : ce qui a été lu et ce qui ne l'a pas été ;
- `sources` : la première est la source principale ; `kind` dit si c'est une loi, un acte signé, un projet, une synthèse, une plaquette, une page officielle, une page tierce ou un service de données.

Textes non lus et donc marqués comme tels : arrêtés signés des APPB Haut-Jura (27/05/2019) et Haut-Doubs (31/12/2025) — règles lues dans les projets, la synthèse et la plaquette ; arrêté municipal de la commune du jeu de test alpin sur le feu (08/07/2022) — connu par la page du PNR du Queyras.

## Champs simples (`fields`)

`skimap_zones::fields::zone_fields(layer, rules, zone, date)` donne ce qu'une carte peut montrer d'une zone : `bivouac` (`forbidden`, `forbidden_in_places`, `tolerated`, `consent_required`, `allowed`, `unknown`) et `access` (`forbidden`, `forbidden_in_places`, `routes_only`, `routes_only_in_places`, `avoid_recommended`, `allowed`, `unknown`), avec la période et les règles qui décident. Une zone est gouvernée par ses règles et par celles des territoires de la couche qui contiennent un de ses points intérieurs (un territoire, par les plus grands seulement) ; verdict le plus restrictif dans l'ordre de `skimap_core::zone_eval`, règles en vigueur à la date ou plus tard. Le pack ajoute ces champs à chaque zone ; tests sur les règles réelles (`fields::tests`) et sur les deux packs (`crates/pack/tests/real_packs.rs`).

## Règles nationales et règles locales (G4)

`skimap_zones::local` s'applique à chaque site, sans rien de propre à un site :

- **Règles nationales** (`Rule::is_national`) : une règle qui ne choisit ses zones que par type ou par pays (`country:*`) : R111-32 et R111-33 du code de l'urbanisme, L131-1 et R163-11 du code forestier, ordonnances fédérales suisses. Elles s'appliquent dès que la couche a ces zones.
- **Acte propre** (`own_act`) : APPB (arrêté préfectoral), réserve naturelle nationale (décret) ou régionale (délibération), cœur de parc national (réglementation du cœur ; l'aire d'adhésion, que l'INPN nomme « [Aire D'Adhésion] », n'en a pas), réserve biologique, captage (arrêté de DUP), commune, département, canton. Natura 2000, PNR, sites, monuments, Biodiv'sports et districts francs n'ont que des règles nationales.
- **Statut** (`local_status`) : `read` quand une règle nomme la zone (son acte a été lu, ses sources disent si c'est le texte signé ou un projet), `partial` pour un territoire dont une partie des arrêtés a été lue, `unverified` sinon : « règles locales non vérifiées ». Les arrêtés municipaux et préfectoraux ne sont jamais recensés : tout site français a donc un statut global `unverified`.

Sorties : `stats.local_rules` et une note dans `zones.meta.json`, `local_rules` dans `build_report.json` ; champs `local_act` et `local_rules` de chaque zone du pack ; ligne `local_rules` de la fiche d'un lieu (`skimap_ffi::rules::place_rules`, textes fr/en).

```sh
SKIMAP_DATA_DIR=data/public cargo run --release -p skimap-zones --example local_rules
```

Mesure du 17/11/2026 sur les couches publiques (`data/public`) : 268 sites du registre sur 272 ont une couche de zones (4 n'en ont pas) ; 0 site sans règle nationale ; R111-32 et L131-1 sur 268 sites, R111-33 sur 227, R163-11 sur 17, ordonnance sur la chasse (districts francs) sur 6, accès aux forêts suisse (CC 699) sur 26. Zones à acte propre : APPB 26 lues et 212 non vérifiées, réserves naturelles nationales 66 et régionales 11 non vérifiées, cœurs de parc 24 non vérifiés, réserves biologiques 81 non vérifiées, départements 60 en partie et 285 non vérifiés, communes 14 en partie, cantons 17 en partie et 15 non vérifiés. Fiche d'un lieu au point intérieur de chacune des 394 zones à acte non lu des 268 packs : la ligne nomme la zone (`every_pack_names_the_unread_acts`, 0,17 s).

## Biodiv'sports : usage personnel

Biodiv'sports n'affiche aucune licence (docs/licences-donnees.md, « Usage personnel seulement, tant qu'il n'y a pas d'accord écrit : […] Biodiv'sports (aucune licence trouvée) ») : `build_with(…, personal_sources)` ne lit ses zones qu'avec l'option et marque alors sa source `personal_use_only` ; `build` lit l'option dans l'environnement (`SKIMAP_PERSONAL_SOURCES`, comme la ligne de commande). Sans l'option : 28 zones sur le jeu de test alpin (10 de moins), 34 sur le jeu de test jurassien (6 de moins).

## Limites

- Hors de l'emprise du site, rien n'est évalué. Italie : aucune zone ni règle.
- Aucune couche forêt : les règles « à moins de 200 m des bois » (L131-1) et « en forêt et à moins de 10 m des lisières » (Vaud) donnent « interdit par endroits ».
- Zones cœur de l'APPB Haut-Doubs : contour Biodiv'sports n° 686, hors des deux emprises actuelles.
- Licences GPU, Biodiv'sports et geo.api.gouv.fr non vérifiées (F5, #11).
