# Format du pack (P14, #27)

[← skimap-pack](../README.md)

Dossier `data/out/<site>/pack/`, écrit dans un dossier temporaire puis substitué d'un coup (un lecteur ne voit jamais un pack à moitié écrit) :

```text
manifest.json                   manifeste (format skimap-pack, version 1)
pack.meta.json                  le pack décrit dans le schéma commun common::meta::LayerMeta
basemap/plan-ign.pmtiles        tuiles vectorielles Plan IGN v2 (MVT gzip)
basemap/plan-ign/style.json     style IGN « standard », URL relatives au pack
basemap/plan-ign/sprite/…       PlanIgn(.json|.png|@2x)
basemap/plan-ign/fonts/<pile>/<plage>.pbf
basemap/osm.pmtiles             extrait Protomaps (MVT gzip)
basemap/osm/style.json, sprites/, fonts/
layers/<id>.geojson             couches vectorielles (WGS84, coordonnées arrondies à 1e-6°)
layers/<id>.json                données (règles, BERA, synthèses, graphe de pistes, météo)
layers/offpiste-*.skr.gz        rasters du routeur hors piste de core (SKR1, gzip)
layers/slope.pmtiles            polygones des classes de pente (tuiles vectorielles MVT, gzip)
layers/avalanche-exposure.pmtiles  polygones des classes d'exposition aux avalanches (MVT)
layers/meta/<couche>/<couche>.meta.json   copies des métadonnées des couches
layers/snow-*.pmtiles           polygones des classes de neige (MVT)
voice/manifest.json, voice/clips/*.opus, voice/voice.meta.json
```

`manifest.json` :

| Champ | Contenu |
|---|---|
| `format`, `format_version` | `skimap-pack`, `1` ; le lecteur refuse un autre format ou une version plus récente |
| `pack_version` | heure de construction UTC `AAAAMMJJTHHMMSSZ`, croissante |
| `generator`, `generated_at`, `site` | auteur, date, site (identifiant, nom, département, emprise `[O, S, E, N]`, centre) |
| `valid_for` | `until` : la plus proche `valid_until` des couches, `until_layer`, note en français |
| `personal_sources` | si les sources à usage personnel ont été incluses |
| `basemaps[]` | `id`, titre, `tiles` (PMTiles), `style`, zooms, nombre de tuiles, taille, date des données, sources, notes |
| `layers[]` | `id`, titre, `kind` (`vector`, `raster`, `data`), `format` (`geojson`, `json`, `pmtiles`, `skr+gzip`, `tsv`), `file`, `meta`, `origin` (fichier du pipeline), `generated_at`, `valid_from`, `valid_until`, `validity`, `sources` (`common::meta::SourceRef` tels quels), `personal_use_only`, taille, nombre d'entités et types de géométrie, `visible`, `raster`, `classes` (couches de classes en vecteur), notes |
| `layers[].classes` | zooms, `extent`, nombre de tuiles, `source_layer`, `property` (`class`), légende (`value`, `label`, couleur de référence du thème clair), raster source, méthode, seuils des petites taches, tolérance et sommets par zoom, nombre de polygones, surfaces par classe (`classes` : raster, après lissage, polygones par zoom) et par seuil (`levels` : surface perdue, gagnée, polygones, zones significatives conservées) |
| `layers[].raster` | zooms, taille de tuile, nombre de tuiles, `encoding` (`palette_index` : l'indice de palette est la classe ; `rgba_legend` : tuiles tierces), `nodata_index`, légende (`value`, `label`, `color` `#rrggbbaa`), raster source, méthode de sous-échantillonnage |
| `voice` | emplacement des clips (VO3) : `status` (`present`, `absent`), `manifest`, `clips_dir`, nombre, taille, sources |
| `missing[]` | couches cherchées et absentes, avec la raison (absente du pipeline, source à usage personnel sans l'option, outil manquant) |
| `attributions[]` | attributions à afficher, sans doublon |
| `files[]`, `total_bytes` | chaque fichier (sauf le manifeste) avec taille et SHA-256 ; `Pack::verify` les recontrôle |

Toutes les URL des styles et tous les chemins du manifeste sont relatifs à la racine du pack (`pmtiles://basemap/osm.pmtiles`, `basemap/osm/fonts/{fontstack}/{range}.pbf`) ; le visualiseur (et plus tard l'app) les rend absolus. Le lecteur refuse un chemin qui sort du pack.

## Couches reprises

Lues dans `data/out/<site>/` avec leurs `<couche>.meta.json` (schéma `common::meta::LayerMeta` ; les métadonnées hors schéma — `voice`, `offpiste`, `routes` au 15/11/2026 — sont lues avec tolérance et une note le signale) :

| Couche du pack | Origine | Remarque |
|---|---|---|
| `pistes`, `pistes-graph-edges`, `pistes-graph-nodes` | `pistes/` | `pistes` : état et plan des pistes joints voie par voie (voir plus bas) |
| `domaines` | `domaines/` | |
| `zones` (+ `zones-rules`) | `zones/` | propriétés ajoutées : `rules` (règles de `rules.json` qui s'appliquent à la zone, jointure `skimap_zones::Rule::applies_to_zone`), `rules_text` (texte français) et les champs simples de `skimap_zones::fields` : `bivouac`, `access`, leurs périodes et règles décisives (voir plus bas) |
| `no-network`, `coverage-area` | `coverage/` | |
| `avalanche-areas`, `avalanche-crossings`, `avalanche-summary` | `avalanche-historic/` | vides sur le jeu de test jurassien (pas de CLPA) |
| `bera` | `bera/` | |
| `refuges-info`, `refuges-osm`, `refuges-camptocamp`, `refuges-links` | `refuges/` | trois fichiers, pas de fusion (licences, L3) |
| `status`, `status-summary` | `status/` | usage personnel : seulement avec `personal_sources` |
| `meteo-hourly` | `meteo-hourly/` | usage personnel |
| `routes`, `route-offpiste-*`, `piste-graph` | `routes/` | itinéraires de preuve (C1, C3, C5), graphe de pistes typé du noyau |
| `offpiste-elevation`, `-slope`, `-zones`, `-pistes`, `-exposure`, `-zone-classes`, `-snow` | `offpiste/` | rasters SKR1 de core, gzip ; neige : la date la plus récente ; `-exposure` : exposition aux avalanches, surcouche par défaut du routeur |
| `avalanche-segments` | `avalanche-exposure/segments.geojson` | tronçons des pistes par classe d'exposition (« terrain peu exposé (modèle) », « exposé », « très exposé ») ; 275 ko sur le jeu de test alpin, 810 ko sur le jeu de test jurassien (sommets des pistes OSM, plus un sommet interpolé par changement de classe) |
| `assist-briefing`, `assist-wax`, `assist-snow-quality`, `assist-suggestions` | `assist/` (`briefing_<date>.json`, `wax_`, `snow_quality_`, `suggestions_<date>.geojson`) | sorties de `skimap assist` de la dernière date ; usage personnel (elles lisent la météo Open-Meteo et l'état des pistes) : seulement avec `personal_sources` |
| `slope` | `terrain/slope_5m.tif` | polygones vectoriels z10-13, classes de la carte des pentes IGN (`class` 1 à 4 : 30-35, 35-40, 40-45, 45° et plus) |
| `avalanche-exposure` | `avalanche-exposure/exposure_5m.tif` (UInt8 1 / 2 / 3, 255 absente) | polygones vectoriels z10-13, `class` 1 « terrain peu exposé (modèle) », 2 « exposé », 3 « très exposé » ; jamais « sûr » |
| `snow-depth`, `snow-groomed`, `snow-srn` | `snow/*.tif` (25 m) | polygones vectoriels z10-13 (`build::class_tiles`, sans lissage : le côté prudent est moins de neige), `class` 0.. comme les classes de la crate snow ; 99,1 à 99,97 % des pixels redessinés au zoom 15 dans la classe de la source (jeu de test alpin, jeu de test jurassien) |
| `piste-labels` | `layers/pistes.geojson` du pack | une ligne par itinéraire (relation OSM, sinon plan, sinon nom de la voie), voies jointes bout à bout : noms des pistes (`name`, `difficulty`, `length_m`, `ways`) |
| `refuges` | `refuges/refuges.geojson` | fiches fusionnées (CC BY-SA 4.0) : téléphones, accès en hiver, sites, liens refuges.info et Camptocamp, attribution |
| `stations`, `stations-pois`, `stations-status` | `france/stations/` | fiches des domaines de l'emprise (secours, 112 en premier, vente, contacts), points OSM rattachés, fraîcheur (ODbL, SkiRadar) |
| `fee-zones` | `france/stations/titres.geojson` + `couverture.json` | usage personnel : titres valables sur un domaine de l'emprise, schéma de l'app (`fee_id`, `name`, `issuer`, `kind`, `prices`) + `scope`, `season`, `status`, `domains`, `ways` |
| `ign-slopes` | WMTS `SLOPES.MOUNTAIN` (cache de terrain) | comparaison, tuiles RGBA z11-15 telles que servies |

## Couches de classes en vecteur (K15, #123)

La pente et l'exposition aux avalanches ne sont plus des images : `build::class_tiles` en fait des polygones de classes, découpés en tuiles vectorielles (MVT) par zoom dans `layers/<id>.pmtiles`, que les styles colorent dans les teintes du thème (clair ou sombre). Les classes et les seuils sont ceux des rasters du pipeline (pente IGN 30, 35, 40, 45° ; codes 1, 2, 3 de l'exposition), rien n'est recalculé.

Méthode, sur la grille 5 m en Lambert-93 :

1. **Lissage** : chaque seuil « classe ≥ k » reçoit les cellules dont au moins 5 des 9 cellules de la fenêtre 3 × 3 sont au-dessus du seuil (majorité qui ajoute seulement : aucune cellule raide de la source n'est retirée par le lissage, les encoches et cellules isolées en creux sont comblées) ; les seuils restent emboîtés.
2. **Petites taches** : au premier seuil (pente ≥ 30°, exposé ou plus), les taches de moins de **200 m²** (8 cellules) disparaissent et les trous de moins de 200 m² sont comblés ; aux seuils suivants (≥ 35°, ≥ 40°, ≥ 45°, très exposé), **400 m²** (16 cellules) : une tache retirée y reste dans la classe du dessous, toujours dessinée (un îlot de 40-45° dans du 35-40° devient 35-40°). Justification : 200 m² est le cinquième du tamis de 1000 m² des zones de départ du modèle d'exposition (`gdal_sieve`, crates/avalanche), 400 m² moins de la moitié ; mesuré sur les deux sites, **aucune zone significative** (zone 4-connexe de la source d'au moins 1000 m²) ne perd la moindre cellule (couverture minimale 1,000 à chaque seuil, 0 zone sous la moitié), la plus grande zone supprimée fait 175 m² au premier seuil, 375 m² aux suivants. Avec la majorité ordinaire (qui retire aussi) des bandes raides d'une cellule de large disparaissaient : couverture minimale 0,46 et −8,6 % de surface ≥ 30° sur le jeu de test jurassien ; écartée.
3. **Frontières partagées** : les frontières entre régions (4-connexes, même classe) sont coupées en chaînes entre jonctions ; une chaîne passe par les milieux des côtés de ses cellules (l'escalier des cellules est divisé par deux) et est simplifiée une fois par zoom (Douglas-Peucker, extrémités fixes). Deux polygones voisins utilisent la même chaîne : ni trou ni recouvrement, à tous les zooms.
4. **Tuiles** : tolérance d'un zoom = un pixel d'écran au zoom suivant (tuiles de 512 px), au moins 2,5 m (z13 : 3,4 m ; z10 : 27 m) ; un anneau de moins d'un pixel d'écran au carré est omis sous le zoom maximal ; découpe par tuile avec une marge de 64 unités sur 4096 (pas de couture visible) ; une entité par classe et par tuile, attributs `class` et `label`. Zooms 10 à 13, au-delà la carte agrandit les tuiles z13 (unité 0,84 m).

Mesures (15/11/2026, `cargo run --release -p skimap-pack --features pipeline --example class_tiles`, et le manifeste) :

| Seuil | Jeu de test alpin source | perdu / gagné (cellules) | polygones z13 | Jeu de test jurassien source | perdu / gagné | polygones z13 |
|---|---|---|---|---|---|---|
| pente ≥ 30° | 99,94 km² | 0,66 % / 2,28 % | +1,03 % | 7,131 km² | 3,13 % / 1,74 % | −4,07 % |
| ≥ 35° | 55,64 km² | 2,31 % / 4,19 % | +2,02 % | 3,116 km² | 7,34 % / 2,04 % | −4,58 % |
| ≥ 40° | 25,97 km² | 4,03 % / 5,92 % | +1,45 % | 1,170 km² | 10,45 % / 1,69 % | −7,07 % |
| ≥ 45° | 14,21 km² | 6,46 % / 6,06 % | +2,92 % | 0,571 km² | 10,73 % / 1,57 % | +0,15 % |
| exposé ou très exposé | 228,70 km² | 0,00 % / 0,43 % | +0,47 % | 26,94 km² | 0,01 % / 1,59 % | +0,31 % |
| très exposé | 97,97 km² | 0,03 % / 0,82 % | +0,90 % | 0,211 km² | 0,05 % / 1,65 % | +1,60 % |

Les surfaces par seuil des polygones sont égales à celles de la source à 7,1 % près au plus (≥ 40° sur le jeu de test jurassien, où les ressauts du Jura sont des bandes étroites : les îlots de moins de 400 m² y passent en 35-40°), le plus souvent à 1 à 3 % près. Le test `real_packs::class_polygons_match_their_geotiffs` redessine les tuiles du pack au zoom 15 (rastériseur de l'export) et compare à la classe de la cellule sous chaque pixel (un pixel sur neuf) : 93,3 % (pente jeu de test alpin, terrain très découpé), 98,0 % (exposition jeu de test alpin), 99,1 % (pente jeu de test jurassien), 98,8 % (exposition jeu de test jurassien) des pixels dans la classe de la source ; écart de chaque seuil sous 8 %.

| Couche | Raster PNG avant (z10-15) | Vecteur (z10-13) | Polygones | Sommets z13 |
|---|---|---|---|---|
| Pente jeu de test alpin | 2,85 Mo | 3,58 Mo | 84 154 | 903 954 |
| Exposition jeu de test alpin | 1,28 Mo | 1,16 Mo | 4 437 | 292 097 |
| Pente jeu de test jurassien | 0,49 Mo | 0,38 Mo | 14 180 | 103 300 |
| Exposition jeu de test jurassien | 0,79 Mo | 0,77 Mo | 8 777 | 171 417 |

Temps (jeu de test alpin, la plus lourde) : lecture 0,1 s, lissage et taches 0,2 s, mesures 0,1 s, chaînes 0,25 s, simplification et tuiles 0,6 s, écriture 0,1 s : 1,5 s pour la pente, 0,7 s pour l'exposition (0,74 s auparavant pour les cinq rasters PNG).

Consommateurs : les styles (`styles/`), le visualiseur, l'export Locus / OsmAnd qui dessine ses propres surcouches PNG depuis ces polygones (`export::class_raster_tiles`). Le routeur hors piste de core lit ses rasters SKR1 (`offpiste-slope`, `offpiste-exposure`), pas ces couches. `Pack::raster_value_at` ne sert plus qu'aux rasters de neige.

## Pistes : état et plan des pistes (K15, P22)

`build::pistes` complète chaque voie OSM de `pistes` (jointure par `osm_way_id`) :

- **État** (couche `status`, chaque source y nomme les voies OSM qu'elle couvre, `osm_way_ids`) : `state` (`open`, `closed`, `unknown`), `state_at` (date de l'état publiée par la source : `update_datetime` ENJ, « Mise à jour » Nordic France datée, date de la page de la mairie), `state_source`, `state_itineraries`. Une voie de plusieurs itinéraires est ouverte si l'un l'est (le tronçon est damé pour lui), fermée sinon si l'un l'est ; seuls comptent les itinéraires de sa pratique (ski pour une voie de ski). Le style colore ainsi le tracé exact des pistes, plus une couche décalée.
- **Plan des pistes officiel** (`plan_pistes/`, P22) : `colour_source`, `oneway_source`, `name_source` (`osm` ou `plan`) ; une couleur du plan colorie une voie sans couleur OSM, une couleur OSM est gardée (le plan ne transcrit qu'une partie des pistes : sur une voie, ses couleurs peuvent oublier une piste), une couleur du plan absente d'OSM est gardée à part (`plan_colours`) ; le sens lu sur les flèches du plan rend une voie à sens unique quand OSM ne dit rien (géométrie retournée pour `backward`, `geometry_reversed`) ; `plan_pistes` liste les pistes du plan sur la voie. Tronçons partagés : le « double trait noir/rouge » de la démo P22 vient de ses `MultiLineString` superposées, une par piste du plan. Ici chaque voie est dessinée une fois, dans la couleur de la règle de la couche `pistes` (`difficulty` = la plus facile, une boucle se skie en entier à sa couleur ; `difficulty_max` garde la plus dure) : sur le jeu de test jurassien, 166 voies portent plusieurs pistes du plan de couleurs différentes ; les dessiner dans la plus difficile couperait les boucles vertes et bleues en tronçons noirs, et un trait multiple à ces échelles devient illisible. La fiche donne toutes les pistes du tronçon.
- Ces deux sources sont à usage personnel : jointes seulement avec `personal_sources`, leurs sources sont alors ajoutées à la couche (marquée usage personnel). Sans l'option, toutes les voies sont `unknown`.

Mesures (packs du 15/11/2026) : Jeu de test alpin 100 voies fermées, 96 inconnues (pistes du widget sans voie OSM, sentiers) ; Jeu de test jurassien 229 fermées, 17 ouvertes (Le Glacier, Les Bornes), 445 inconnues (itinéraires ENJ hors de l'emprise ou sans voie OSM le long) ; couleurs du plan conformes à OSM sur 71 et 314 voies, aucune en désaccord ; 18 voies rendues à sens unique par le plan sur le jeu de test jurassien.

## Zones : champs simples dérivés des règles

`skimap_zones::fields` donne pour chaque zone ce qu'une carte peut montrer, à partir de ses règles et de celles des territoires qui la contiennent (pays, département ou canton, commune, au point intérieur de la zone ; un territoire n'est gouverné que par les plus grands) ; verdict le plus restrictif dans l'ordre du noyau (`zone_eval`), règles en vigueur à la date de construction ou plus tard, à un moment quelconque de l'année (la période est donnée) :

- `bivouac` : `forbidden`, `forbidden_in_places` (la règle ne vise qu'une partie sans contour, terrains privés traversés par les pistes), `tolerated`, `consent_required` (R111-32 partout ailleurs en France, Vaud), `allowed`, `unknown` (hors des territoires de la couche) ; `bivouac_period`, `bivouac_rules` ;
- `access` : `forbidden`, `forbidden_in_places`, `routes_only` (sortie des itinéraires interdite), `routes_only_in_places`, `avoid_recommended` (Biodiv'sports, mois sensibles), `allowed`, `unknown` ; `access_period`, `access_rules`.
- `local_act` : acte propre de la zone en plus des règles nationales (`skimap_zones::local`, G4) : `arrete_prefectoral_biotope`, `decret_reserve_nationale`, `deliberation_reserve_regionale`, `reglementation_coeur_parc` (cœur seulement, pas l'aire d'adhésion), `arrete_reserve_biologique`, `arrete_captage`, `arretes_municipaux`, `arretes_prefectoraux`, `droit_cantonal` ; null pour les zones régies par les seules règles nationales (Natura 2000, PNR, sites, monuments, Biodiv'sports, districts francs) ;
- `local_rules` : `read` (une règle nomme la zone : son acte a été lu, ses sources disent si c'est le texte signé), `partial` (territoire dont une partie des arrêtés a été lue), `unverified` (règles locales non vérifiées) ; null sans acte propre.

Exemples réels : APPB Haut-Jura (`inpn:FR3800988`) bivouac interdit et itinéraires seulement du 15 décembre au 30 juin ; APPB Forêt du Paradis bivouac interdit toute l'année ; Doubs bivouac interdit par endroits ; France bivouac avec l'accord du propriétaire ; Arvieux bivouac toléré.

## Sources à usage personnel

Un pack construit sans `personal_sources` ne contient aucune source à usage personnel ni non autorisée (`build::layers::unauthorised` : marquée `personal_use_only`, ou reconnue par son nom même dans une ancienne métadonnée : Biodiv'sports, CLPA, EPA, ENJ, Nordic France, pages « Info damage » des mairies, Open-Meteo ; docs/licences-donnees.md, « Usage personnel seulement, tant qu'il n'y a pas d'accord écrit »). Une couche calculée à partir d'une couche personnelle l'est aussi (`Spec::depends` : rasters hors piste et itinéraires hors piste de core depuis l'exposition et les zones) ; les clips de voix qui disent des noms d'une source personnelle sont écartés. Tests : `synthetic_pack::public_pack_has_no_restricted_source` (sorties construites avec l'option, métadonnée ancienne non marquée) et `real_packs::public_pack_from_real_outputs_has_no_restricted_source` (sorties réelles) : sources et attributions du manifeste, métadonnées copiées, contenu des fichiers.

## Rasters

Tout GeoTIFF Float32 du pipeline (`common::geotiff`, projection quelconque de `common::crs`) devient une couche par une entrée de `build::raster::catalog()` : classes (bornes, libellés, couleurs), zooms, sens des égalités. Au zoom maximal, chaque pixel prend la classe de la cellule source sous son centre (projection exacte pixel par pixel, rayon parallèle) ; les zooms inférieurs prennent la classe la plus fréquente des 2 × 2 pixels (égalité : la plus haute pour la pente, la plus basse pour la neige, le côté prudent). PNG 8 bits à palette : l'indice est la classe, la dernière entrée transparente est « pas de donnée ». `Pack::raster_value_at(id, position)` relit la classe au zoom maximal (décodeur PNG de la crate, sans dépendance). Les rasters de la neige passeront tels quels quand snow écrira des centimètres entiers : la classe se calcule sur la valeur.

## Tailles des packs (15/11/2026, 10 h 36, pente et exposition en vecteur)

Packs construits avec `personal_sources` (usage personnel de l'auteur) et sans les tuiles IGN de comparaison ; entre parenthèses, les mêmes sorties construites à 9 h 39 avec la pente et l'exposition en rasters PNG :

| Partie | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Fond Plan IGN v2 (z0-16, style, sprite, glyphes) | 13,13 Mo | 12,95 Mo |
| Fond OSM Protomaps (z0-15, style, sprite, glyphes) | 4,62 Mo | 5,58 Mo |
| Couches `layers/` | 23,73 Mo (18,95) | 18,10 Mo (16,80) |
| dont rasters hors piste de core (gzip) : altitude / pente | 8,53 / 5,73 Mo | 5,67 / 4,25 Mo |
| dont pente et exposition en polygones (MVT) | 4,74 Mo | 1,15 Mo |
| Rasters en tuiles (neige z9-13 ; avant : + pente et exposition z10-15) | 0,22 Mo (4,35) | 0,10 Mo (1,38) |
| Voix (clips Opus) | 4,07 Mo | 5,07 Mo |
| **Total** | **45,79 Mo** (45,14), 986 fichiers | **41,82 Mo** (41,79), 1 225 fichiers |
| Couches à usage personnel incluses (zones avec Biodiv'sports, exposition avec la CLPA et ce qui en dérive, état, plan, météo, assistant) | 18,79 Mo | 13,19 Mo |
| Option tuiles IGN des pentes (z11-15, RGBA) | +22,63 Mo | +13,17 Mo |

L'altitude 5 m du routeur hors piste pesait un tiers du pack : core écrit maintenant ses rasters SKR1 en différences le long des lignes (octets faibles puis forts), et le gzip de l'altitude passe de 17,1 à 8,6 Mo sur le jeu de test alpin, de 10,5 à 5,7 Mo sur le jeu de test jurassien (zlib 6). Le format SKR1 appartient à core (`crates/core/docs/horspiste.md`).

Temps de construction depuis les caches : 2,2 s (jeu de test alpin) et 1,8 s (jeu de test jurassien) avec les rasters PNG ; 4,0 s et 3,2 s avec les polygones (poste chargé par d'autres compilations : jusqu'à 6,3 s et 4,1 s), dont 1,5 s et 0,65 s pour la pente, 0,7 s et 1,0 s pour l'exposition.

## Contrôles

- `tests/synthetic_pack.rs` : pack construit sur des sorties synthétiques sans réseau, relu, exporté ; sources à usage personnel écartées sans l'option.
- `tests/real_packs.rs` (données réelles, `--ignored`) : `Pack::verify` sans écart ; chaque couche GeoJSON garde toutes les entités de sa source ; jointure zones-règles identique à la liste de la crate zones (`zones.meta.json` : 23 paires sur le jeu de test alpin, 33 sur le jeu de test jurassien) ; tous les pixels contrôlés (un sur neuf) des tuiles de neige au zoom maximal égaux à la classe de la cellule GeoTIFF sous leur centre (0 écart sur 0,27 M par couche) ; polygones de pente et d'exposition redessinés au zoom 15 comparés aux GeoTIFF (voir plus haut) ; champs `bivouac` et `access` des zones sur les règles réelles ; état sur chaque voie nommée par un itinéraire de ski ; pack public sans source personnelle.
