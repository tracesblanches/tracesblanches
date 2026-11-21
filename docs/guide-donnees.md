# Guide des données

[← README](../README.md)

Ce que contient un pack, d'où vient chaque couche, combien de temps elle vaut et comment la reconstruire. Détail des formats : [architecture/donnees.md](architecture/donnees.md) (arborescence, configuration, schéma des métadonnées), [crates/pack/README.md](../crates/pack/README.md) (format du pack). Licences : [licences-donnees.md](licences-donnees.md).

## Du téléchargement au pack

1. `skimap build-all <site>` télécharge chaque source une fois dans `data/cache/<source>/` (quotas par source, reprise, `ETag` et `Last-Modified` : [telechargeur.md](architecture/telechargeur.md)).
2. Chaque crate écrit sa couche dans `data/out/<site>/<couche>/`, avec un `<couche>.meta.json` : sources (nom, adresse, licence, attribution, `personal_use_only`), dates (`generated_at`, `valid_from`, `valid_until`, `validity`), fichiers, paramètres, chiffres clés.
3. `build-pack` assemble `data/out/<site>/pack/` : `manifest.json`, `pack.meta.json`, `basemap/` (Plan IGN v2 et fond OSM en PMTiles, polices, icônes), `layers/` (GeoJSON, tuiles vectorielles de classes, rasters SKR1 du hors piste, JSON), et relit le tout avec le lecteur du téléphone.
4. `skimap-server publish` en fait des paquets immuables et des mises à jour différentielles ([crates/server/README.md](../crates/server/README.md)).

## Le manifeste

`manifest.json` décrit le pack pour l'app :

- `layers[]` : une entrée par couche (`id`, `title`, `kind` `vector` ou `data`, `file`, `meta`), avec ses `sources` et ses dates ;
- `missing[]` : chaque couche absente et la raison (« source à usage personnel seulement : option personal_sources désactivée », « … absent des sorties du pipeline ») ; une couche absente n'est jamais remplacée par une valeur inventée ;
- `attributions[]` : les mentions à afficher, avec la date de mise à jour de chaque source ;
- `valid_for` : la date avant laquelle reconstruire le pack et la couche qui l'impose ;
- `personal_sources` : `false` pour un pack publiable.

Exemple mesuré le 21/11/2026 sur le jeu de test jurassien, sans sources à usage personnel : 37 couches, 14 absentes, 31 attributions, pack valable jusqu'au 23/11/2026 (couche `meteo-hourly`), 33,47 Mo.

## Les couches

| Famille | Couches du manifeste | Sources (`sources[]` des couches) | Validité (`validity`) |
|---|---|---|---|
| Fonds | `plan-ign`, `osm` (dans `basemaps`) | Plan IGN v2 ; OpenStreetMap (Protomaps) | édition de la source |
| Pistes et domaines | `pistes`, `piste-labels`, `domaines`, `pistes-graph-*`, `piste-graph` | extrait OSM d'OpenSnowMap, OpenSkiMap, contours des communes ; relief LiDAR HD pour le graphe | réseau OSM à la date de l'extrait |
| SRN | `srn-areas`, `srn-routes` | extrait OSM d'OpenSnowMap, DATAtourisme, recensement des domaines | voir `.meta.json` |
| Relief | `slope`, `offpiste-elevation`, `offpiste-slope` | MNT LiDAR HD de l'IGN (5 m) ; carte des pentes IGN en comparaison seulement | relief permanent (date des levés) |
| Avalanches | `avalanche-exposure`, `avalanche-segments`, `offpiste-exposure`, `bera` | modèle calculé (méthode AutoATES v3.0, écoulement Flow-Py) sur le MNT et le MNH LiDAR HD ; liens vers les bulletins Météo-France et SLF | modèle : celle du relief ; BERA : le jour |
| Neige | `snow-depth`, `snow-groomed`, `snow-srn`, `offpiste-snow` | simulation nivologique de Météo-France, contours des massifs (snowtools) | le jour de la simulation |
| Météo et assistant | `meteo-hourly`, `assist-*` | paquets AROME de Météo-France ; l'assistant combine les couches ci-dessus | quelques jours (`valid_until`) |
| Refuges | `refuges`, `refuges-info`, `refuges-camptocamp` | refuges.info, Camptocamp | fiches datées |
| Zones | `zones`, `zones-rules`, `offpiste-zones`, `offpiste-zone-classes` | Géoportail de l'urbanisme, INPN, geo.admin.ch, contours des communes | à régénérer au moins chaque mois |
| Réseau mobile | `no-network`, `coverage-area` | ARCEP (Mon Réseau Mobile), contours des communes | publication semestrielle de l'ARCEP |
| Stations | `stations`, `stations-pois`, `stations-status` | OpenSkiMap, OpenStreetMap, recensement des domaines, SkiRadar | fiches relues tous les 7 jours |

La validité exacte de chaque couche est dans son `.meta.json` (`valid_until`, `validity`) ; l'app affiche « Mis à jour le… » pour une donnée qu'elle n'a pas pu rafraîchir.

## Sources à usage personnel

Certaines sources n'autorisent pas la redistribution (état des pistes publié sans licence, API gratuite d'Open-Meteo, CLPA et EPA d'INRAE, pages des émetteurs de titres). Elles ne sont lues qu'avec `SKIMAP_PERSONAL_SOURCES=1`, portent `personal_use_only: true` dans les métadonnées, et un pack qui en contient a `personal_sources: true` : il ne se publie pas. Sans l'option, les étapes concernées sont ignorées (`status-poll`) ou écrivent leur couche sans ces sources.

## Reconstruire et rafraîchir

- Un site : `skimap build-all <site>` ; une étape seule : la sous-commande de sa couche (`skimap snow <site>`, `skimap refuges <site>`…), puis `skimap build-pack <site>`.
- Toute la France : `skimap build-all --all` ; rafraîchissement planifié des couches qui vieillissent : `skimap refresh` ([architecture/france.md](architecture/france.md)).
