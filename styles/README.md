# Styles des fonds de carte par pratique (K15, #123)

Trois styles MapLibre natifs, chacun en clair (plein soleil, neige) et en sombre (aube, nuit), construits sur les **mêmes fichiers du pack** : tuiles vectorielles Plan IGN v2 et Protomaps (OSM), couches GeoJSON et rasters de classes. Aucune tuile supplémentaire. Un seul jeu de fichiers pour le visualiseur (MapLibre GL JS) et l'app (MapLibre Native Android).

| Fichier | Pour | Couches | dont IGN | Symboles |
|---|---|---|---|---|
| `fond-clair.json`, `fond-sombre.json` | ski de fond sur pistes damées | 38 / 39 | 18 | 10 |
| `srn-clair.json`, `srn-sombre.json` | ski de randonnée nordique hors piste | 61 / 64 | 29 | 17 |
| `guidage-clair.json`, `guidage-sombre.json` | coup d'œil en mouvement | 17 | 8 | 4 |
| Plan IGN « standard » (comparaison) | | 434 | 434 | 160 |

Les fichiers sont **générés** par la crate `crates/styles` (palettes dans `src/palette.rs`, couches dans `src/generate.rs`) ; ne pas les éditer à la main :

```sh
cargo run -p skimap-styles --example write_styles                         # les six styles
cargo run --release -p skimap-styles --example write_styles -- --schema   # + pack-schema.json (packs requis)
```

## Contenu

- **Fond** : pistes au premier plan (couleur de difficulté, liseré, nom en gras le long du tracé dès z13), état (piste fermée : couleur voilée et tiretée, ouverte : pleine), sens unique (chevrons blancs dès z14), classique seul (trace centrale), hors piste balisé (tirets), raquettes et piétons (pointillé fin) ; exposition « très exposé » sous la piste (bande rayée) ; parkings (P) et accueils nordiques (OSM) ; refuges gardés et gîtes ; relief discret (courbes maîtresses dès z12, normales dès z14) ; communes, hameaux, sommets principaux seulement.
- **SRN** : courbes marquées et cotées, points cotés, forêt fermée et ouverte, rochers, chemins et pistes forestières ; exposition aux avalanches à l'échelle du domaine (z12-13), classes de pente IGN de près (dès z14) ; couloirs d'avalanche connus (CLPA) ; zones protégées utiles en hiver (zones de quiétude Biodiv'sports « terrestre », APPB, réserves, parc national, district franc) et interdictions de bivouac ; refuges, cabanes, abris, points d'eau, passages délicats ; zones sans réseau ; pistes en second plan, étroites.
- **Guidage** : fond minimal (forêt pâle, eau, grandes routes, communes), pistes en gris de contexte, itinéraire du jour (déjà parcouru en gris), prochaine décision (disque jaune cerclé, texte de 20 px), position.

Neige (`snow-groomed` dans fond, `snow-srn` dans SRN) : couches présentes mais masquées (`metadata.skimap:optional_layers`), à afficher sur demande.

## Adresses relatives au pack

Comme les styles des fonds du pack, toutes les adresses sont relatives à la racine du pack : `pmtiles://basemap/plan-ign.pmtiles`, `pmtiles://rasters/slope.pmtiles`, `layers/pistes.geojson`, `basemap/plan-ign/fonts/{fontstack}/{range}.pbf`, `basemap/plan-ign/sprite/PlanIgn`. Le visualiseur les rend absolues (`tools/viewer`), l'app les réécrit en `file://<pack>/…` (et `pmtiles://file://<pack>/…`). Polices : Source Sans Pro du pack, une seule par couche ; icônes : sprite Plan IGN (refuges).

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
- Rasters de classes : `raster-resampling: nearest` (pixels nets), sous tous les tracés et étiquettes.

## Validation (`cargo test -p skimap-styles`)

- Les styles du dépôt sont ceux du générateur.
- Validateur (`src/validate.rs`) contre `pack-schema.json`, inventaire des deux packs (couches des tuiles, attributs et valeurs comparées, couches GeoJSON, rasters, polices, icônes) : JSON MapLibre valide ; chaque source pointe vers un fichier de **chaque** pack ; aucune source sans couche ; chaque `source-layer` existe aux zooms où la couche est visible ; chaque propriété lue existe ; chaque valeur comparée existe (ou est déclarée par la crate productrice, ex. les types de zones de `skimap-zones`) ; type de couche compatible avec les géométries ; polices et icônes présentes ; sous-ensemble d'expressions commun à GL JS et Native, pas d'ancienne syntaxe des filtres ; ordre et netteté des rasters ; contrastes.
- `cargo test --release -p skimap-styles -- --ignored` (packs réels `data/out/<site>/pack`) : mêmes contrôles sur l'inventaire frais, **aucune couche orpheline** (chaque couche dessine au moins une entité d'un des deux packs, filtres évalués sur toutes les tuiles), `pack-schema.json` à jour.

## Limites

- L'état des pistes est une couche à part (`status`) dont la géométrie ne recouvre pas exactement celle des pistes ; les itinéraires de raquettes sont écartés par la pratique.
- Les interdictions de bivouac sont lues dans le texte des règles (`rules_text` contient « Bivouac : interdit »).
- Les couleurs des rasters de pente et d'exposition sont figées dans la palette PNG du pack : en sombre, elles restent celles du thème clair.
