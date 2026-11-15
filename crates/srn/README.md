# skimap-srn

Domaines de jeu du ski de randonnée nordique (SRN) de toute la France hors stations damées : traversées (GTJ, Hauts-Plateaux du Vercors…), plateaux, itinéraires balisés. Ticket : G5 (#131).

- [Méthode, sources et licences, résultats, vérifications, limites](docs/srn.md)

API : `skimap_srn::build(&layout, &downloader)` (lit l'extrait OpenSnowMap), `skimap_srn::build_from(&layout, &downloader, &planet)` pour réutiliser l'extrait déjà lu (`skimap_osm::extract::load_planet`). Lit le recensement des domaines de fond (P3, `data/out/france/domaines/domaines.geojson`) ; le calcule s'il manque.

```sh
cargo run --release -p skimap-srn --example srn
```

## Sorties (`data/out/france/srn/`, WGS84)

| Fichier | Contenu |
|---|---|
| `srn_areas.geojson` | une zone par entité, MultiPolygon : `domaine` (itinéraires fermés à 750 m, puis couloir de 500 m) ou `traversee` (couloir de 500 m) |
| `srn_routes.geojson` | itinéraires des zones, LineString ou MultiLineString, avec provenance et licence |
| `srn_hints.geojson` | points : documents qui décrivent un itinéraire sans tracé (à compléter) |
| `srn.meta.json` | sources, licences, propriétés documentées, paramètres, chiffres (`stats`) |

Propriétés d'une zone : `id`, `name`, `name_source`, `kind`, `evidence` (`srn` ou `compatible`), `massif`, `commune` (`name`, `insee`, `departement`), `lon`, `lat`, `area_km2`, `km_routes`, `km_attached`, `routes`, `route_names`, `attached`, `providers`, `licences`, `stations` (domaines damés recoupés : `id`, `name`, `area_share`, `km_inside`), `crosses`, `length_km` et `published_length_km` (traversées).

## Sources

| Source | Ce qui est pris | Licence |
|---|---|---|
| OSM (extrait OpenSnowMap) | `piste:type=nordic` + `piste:grooming=backcountry` (SRN) ; `skitour`, `hike`/`snowshoe` (rattachés) | ODbL 1.0 |
| Geotrek (81 instances de la liste officielle) | pratiques nommées « nordique » : PNR du Vercors seulement | non affichée (voir docs) |
| Camptocamp | `skitouring` cotation 1.1 à 1.3 ; titres « nordique », « ski de fond » | CC BY-SA 3.0 |
| DATAtourisme | tours « nordique » au thème `SkiTouring`, « ski de randonnée », avec le GPX de la fiche | Licence Ouverte 2.0 |
| P3, geo.api.gouv.fr | domaines damés ; commune, département, France | ODbL ; Licence Ouverte 2.0 |

Requêtes polies par le téléchargeur commun : 1 par seconde et par source, cache de 30 jours (7 jours pour le CSV DATAtourisme).
