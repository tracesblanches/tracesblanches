# Styles des fonds de carte par pratique (K15, #123)

Trois styles MapLibre natifs, chacun en clair (plein soleil, neige) et en sombre (aube, nuit), construits sur les **mêmes fichiers du pack** : tuiles vectorielles Plan IGN v2 et Protomaps (OSM), couches GeoJSON, polygones vectoriels de pente et d'exposition aux avalanches (`layers/slope.pmtiles`, `layers/avalanche-exposure.pmtiles`), rasters de neige (masqués par défaut). Aucune image de classes : tout ce qui est dessiné l'est en vecteur, dans les couleurs du thème. Un seul jeu de fichiers pour le visualiseur (MapLibre GL JS) et l'app (MapLibre Native Android).

| Fichier | Pour | Couches | dont IGN | Symboles |
|---|---|---|---|---|
| `fond-clair.json`, `fond-sombre.json` | ski de fond sur pistes damées | 38 / 39 | 18 | 10 |
| `srn-clair.json`, `srn-sombre.json` | ski de randonnée nordique hors piste | 65 / 68 | 29 | 17 |
| `guidage-clair.json`, `guidage-sombre.json` | coup d'œil en mouvement | 17 | 8 | 4 |
| Plan IGN « standard » (comparaison) | | 434 | 434 | 160 |

Les fichiers sont **générés** par la crate `crates/styles` (palettes dans `src/palette.rs`, couches dans `src/generate.rs`) ; ne pas les éditer à la main :

```sh
cargo run -p skimap-styles --example write_styles                         # les six styles
cargo run --release -p skimap-styles --example write_styles -- --schema   # + pack-schema.json (packs requis)
```

## Contenu

- **Fond** : pistes au premier plan (couleur de difficulté, liseré, nom en gras le long du tracé dès z13), état sur le tracé exact de chaque voie (champ `state` de la couche `pistes` : fermée, couleur voilée et tiretée ; ouverte ou inconnue, pleine), sens unique (chevrons blancs dès z14), classique seul (trace centrale), hors piste balisé (tirets), raquettes et piétons (pointillé fin) ; exposition « très exposé » sous la piste (bande rayée) ; parkings (P) et accueils nordiques (OSM) ; refuges gardés et gîtes ; relief discret (courbes maîtresses dès z12, normales dès z14) ; communes, hameaux, sommets principaux seulement.
- **SRN** : courbes marquées et cotées, points cotés, forêt fermée et ouverte, rochers, chemins et pistes forestières ; exposition aux avalanches à l'échelle du domaine (polygones « exposé » et « très exposé », z11-14), classes de pente IGN de près (polygones 30-35, 35-40, 40-45, 45° et plus, dès z13-14) ; couloirs d'avalanche connus (CLPA) ; zones dont les règles restreignent le ski (champ `access` dérivé des règles : accès interdit, itinéraires balisés seulement, à éviter pour la faune) et réserves, parc national ; interdictions de bivouac (champ `bivouac` = `forbidden`) ; refuges, cabanes, abris, points d'eau, passages délicats ; zones sans réseau ; pistes en second plan, étroites.
- **Guidage** : fond minimal (forêt pâle, eau, grandes routes, communes), pistes en gris de contexte, itinéraire du jour (déjà parcouru en gris), prochaine décision (disque jaune cerclé, texte de 20 px), position.

Neige (`snow-groomed` dans fond, `snow-srn` dans SRN) : couches présentes mais masquées (`metadata.skimap:optional_layers`), à afficher sur demande.

## Pente et exposition en vecteur

Polygones de classes (pack, `build::class_tiles`), une couche `fill` par classe, filtre sur l'attribut `class` :

| Couche | Classes | Clair | Sombre |
|---|---|---|---|
| `slope-1` à `slope-4` | 30-35, 35-40, 40-45, 45° et plus | `#f2e500`, `#f39419`, `#f00000`, `#c889bb` (teintes de la carte des pentes IGN), opacité 0,55 | `#e6d200`, `#ff8c1a`, `#ff3b3b`, `#c98bff`, opacité 0,78 (une teinte translucide sur fond noir devient boueuse : plus opaque, plus claire) |
| `exposure-2`, `exposure-3` | exposé, très exposé (« peu exposé » n'est pas dessiné : jamais « sûr ») | `#f59628`, `#c81e1e`, opacité 0,3 | `#ff9f1c`, `#ff2d44`, opacité 0,38 |

Pente de z13 (fondu) à z14 et au-delà (tuiles z13 agrandies), exposition de z11 à z14 (fondus) : à l'échelle du domaine, plus de 90 % de Cervières est « exposé » ou « très exposé », un voile léger suffit.

## Groupes de couches (désactivables)

Chaque couche porte `metadata.skimap:group` ; le style déclare ses groupes dans `metadata.skimap:groups` (`id`, `title`, `toggleable`), dans l'ordre du menu. L'app et le visualiseur affichent un interrupteur par groupe désactivable et ne changent que la visibilité des couches (`visibility`), sans recharger le style ; le choix est mémorisé par style. Les groupes de sécurité et le fond ne se masquent pas. Le validateur refuse une couche sans groupe, un groupe inconnu ou dont le `toggleable` diffère de la liste, un groupe de sécurité désactivable.

| Groupe | Titre | Désactivable | Couches |
|---|---|---|---|
| `fond` | Fond de carte (relief, forêt, eau, routes) | non | `background`, `ign-*` sauf les noms |
| `pistes` | Pistes et leur état | non | `pistes-*` |
| `dangers` | Avalanches et passages délicats | non | `exposure-*`, `avalanche-areas-*`, `avalanche-segments`, `hazard*` |
| `reglementation` | Zones réglementées et bivouac | non | `zones-*` |
| `itineraire` | Itinéraire et prochaine décision | non | `route-*`, `decision*` |
| `position` | Position | non | `position-*` |
| `pentes` | Pentes | oui | `slope-*` |
| `refuges` | Refuges et abris | oui | `huts-*` |
| `points-eau` | Points d'eau | oui | `water-point` |
| `acces` | Parkings et accueils | oui | `osm-parking*`, `osm-centre*` |
| `noms` | Noms de lieux, sommets, cotes | oui | `ign-place-*`, `ign-oro*`, `ign-lieu-dit`, `ign-contour-label`, `ign-spot-height` |
| `reseau` | Zones sans réseau | oui | `no-network-*` |
| `neige` | Neige estimée | oui | `snow-*` |

## Adresses relatives au pack

Comme les styles des fonds du pack, toutes les adresses sont relatives à la racine du pack : `pmtiles://basemap/plan-ign.pmtiles`, `pmtiles://layers/slope.pmtiles`, `layers/pistes.geojson`, `basemap/plan-ign/fonts/{fontstack}/{range}.pbf`, `basemap/plan-ign/sprite/PlanIgn`. Le visualiseur les rend absolues (`tools/viewer`), l'app les réécrit en `file://<pack>/…` (et `pmtiles://file://<pack>/…`). Polices : Source Sans Pro du pack, une seule par couche ; icônes : sprite Plan IGN (refuges).

## Sources remplies par l'app

`metadata.skimap:runtime_sources` déclare les sources vides au chargement, remplies par `setData` :

| Source | Géométrie | Propriétés |
|---|---|---|
| `position` (trois styles) | Point | `accuracy_m` (rayon du disque de précision) |
| `route` (guidage) | LineString | `part` : `ahead` (à faire) ou `done` (fait) |
| `decision` (guidage) | Point | `label` (« À droite dans 690 m ») |

## Lisibilité au soleil

- Clair : fond blanc (neige), couleurs saturées et sombres, liserés blancs ; sombre : fond presque noir, couleurs éclaircies, liserés sombres, piste noire liserée de clair, disque clair sous les icônes IGN.
- Contraste vérifié par le validateur (WCAG) : texte et halo ≥ 4,5:1 et halo ≥ 1,5 px pour toute étiquette ; ≥ 3:1 pour les tracés marqués (`metadata.skimap:contrast`) contre le fond ou leur liseré (pistes, itinéraire, position, décision, zones, réseau, CLPA).
- Pente et exposition : polygones vectoriels nets à tous les zooms, sous tous les tracés et étiquettes ; rasters de neige `raster-resampling: nearest`.

## Validation (`cargo test -p skimap-styles`)

- Les styles du dépôt sont ceux du générateur.
- Validateur (`src/validate.rs`) contre `pack-schema.json`, inventaire des deux packs (couches des tuiles, attributs et valeurs comparées, couches GeoJSON, rasters, polices, icônes) : JSON MapLibre valide ; chaque source pointe vers un fichier de **chaque** pack ; aucune source sans couche ; chaque `source-layer` existe aux zooms où la couche est visible ; chaque propriété lue existe ; chaque valeur comparée existe (ou est déclarée par la crate productrice : types de zones et champs `bivouac`, `access` de `skimap-zones`, `state` des pistes) ; groupe déclaré pour chaque couche ; type de couche compatible avec les géométries ; polices et icônes présentes ; sous-ensemble d'expressions commun à GL JS et Native, pas d'ancienne syntaxe des filtres ; ordre et netteté des rasters ; contrastes.
- `cargo test --release -p skimap-styles -- --ignored` (packs réels `data/out/<site>/pack`) : mêmes contrôles sur l'inventaire frais, **aucune couche orpheline** (chaque couche dessine au moins une entité d'un des deux packs, filtres évalués sur toutes les tuiles), `pack-schema.json` à jour.

## Limites

- Une voie sans état connu (`unknown`) est dessinée comme une piste ouverte : seules les pistes fermées sont voilées.
- Un tronçon partagé par des pistes de couleurs différentes est dessiné dans la plus facile (règle de la couche `pistes`) ; la fiche liste toutes ses pistes.
- Neige : rasters à 25 m, masqués par défaut.
- Captures avant / après (raster contre vecteur, clair et sombre) : `vector-shots/planches/` du poste (rapport de #123).
