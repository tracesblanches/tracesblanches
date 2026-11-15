# skimap-coverage

Zones sans aucun réseau mobile, même pour le 112 (ARCEP, pipeline). Ticket : P12 (#25).

Le 112 passe par l'antenne de n'importe quel opérateur : une zone est sans réseau quand aucun des quatre réseaux ne la couvre. Le calcul fait l'union des couvertures 2G/3G voix d'Orange, SFR, Free et Bouygues (niveaux TBC, BC et CL), puis la différence avec la partie française de l'emprise du site.

```sh
# archives déjà téléchargées (sinon le téléchargeur commun les récupère, source « arcep »)
cargo run --release -p skimap-coverage --example build_coverage -- --archives research/raw/arcep/dl
cargo test -p skimap-coverage --test real_sites -- --ignored     # chiffres comparés au rapport 06
```

## Chaîne

1. Partie française de l'emprise : contours des communes (geo.api.gouv.fr, via `skimap_zones::sources::jurisdiction::france_part`), densifiés à ~50 m puis projetés en Lambert-93.
2. Par opérateur, en parallèle : archive 7z (dossier local ou téléchargeur), extraction du GeoPackage avec `sevenz-rust2` dans `data/cache/arcep/gpkg/` (réutilisée tant que l'archive ne change pas), lecture avec `rusqlite` par l'index R-tree, analyse WKB maison (pas de GDAL), découpe à la zone, union.
3. Union des quatre opérateurs, différence avec la zone.

## Sorties (`data/out/<site>/coverage/`)

| Fichier | Contenu |
|---|---|
| `no_network.geojson` | un polygone par entité, WGS84, propriétés `kind` = `no_voice_network_any_operator`, `area_ha` (Lambert-93 plan), `release` |
| `analysed_area.geojson` | partie française de l'emprise ; au-delà (Italie, Suisse), la couverture est inconnue |
| `coverage.meta.json` | sources, licence (Licence Ouverte), date des cartes, chiffres par opérateur, temps |
| `build_report.json` | mêmes chiffres |

## Résultats (T2 2026)

| | Orange | SFR | Free | Bouygues | Au moins un | Aucun réseau |
|---|---|---|---|---|---|---|
| Cervières (225,6 km²) | 37,7 % | 69,3 % | 54,2 % | 69,4 % | 77,3 % | 51,2 km² (22,7 %), 117 polygones, 79 Ko |
| Chapelle-des-Bois (172,7 km²) | 73,2 % | 84,8 % | 76,9 % | 77,7 % | 89,8 % | 17,6 km² (10,2 %), 100 polygones, 75 Ko |

Identiques au rapport 06 au dixième près.

Avant le 15/11/2026, chaque opérateur était découpé par la zone analysée avant l'union, puis la zone moins l'union : le bord de la zone était calculé une fois par opérateur, avec des arrondis différents, et la différence laissait des milliers de lamelles d'aire nulle (2 740 polygones dont 2 071 vides une fois arrondis au millionième de degré à Cervières, 3 455 dont 2 937 à Chapelle-des-Bois ; 558 et 659 Ko). Le découpage se fait maintenant une seule fois (zone moins l'union des couvertures non découpées) ; les rares morceaux plus étroits que la précision de sortie (deux tracés de la frontière, ARCEP et geo.api.gouv.fr, à quelques millimètres) ne sont pas écrits. Surfaces inchangées (51,25 et 17,64 km², somme des `area_ha` identique à 0,01 ha près).

## Temps (poste de développement, 20 cœurs)

| Cas | Temps total | Dont |
|---|---|---|
| Premier calcul sans rien en cache | 159 s | téléchargement des 4 archives (1 Go, une requête à la fois par source) et extraction ~123 s ; grille geo.api.gouv.fr 35 s ; union et différence 1,1 s |
| Archives locales, extraction à faire | 14,4 s | extraction des 3 Go de GeoPackage 14 s (Free, 973 Mo, est le plus long) |
| Extraction en cache, par site | 0,38 s | union et différence 0,3 s |

Le cas qui domine est le premier calcul de chaque publication (téléchargement puis extraction) ; le calcul géométrique reste sous la seconde par site.

## Limites

- Cartes théoriques des opérateurs : la forêt ou un fond de combe peuvent couper un réseau annoncé.
- Free n'a plus de carte 2G depuis 2024 T4 : sa carte 2G/3G voix est sa 3G.
- Les GeoPackages extraits occupent 3 Go dans `data/cache/arcep/gpkg/`.
