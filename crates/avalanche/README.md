# skimap-avalanche

Exposition aux avalanches : oracle auto-ATES, réimplémentation Rust, validation CLPA / EPA (pipeline). Tickets : P16 (#29), P17 (#30), P18 (#31), P20 (#99).

## Avalanches historiques et tronçons exposés (P20, module `historic`)

`skimap_avalanche::historic::build(site, &layout, &downloader)` écrit `data/out/<site>/avalanche-historic/` :

| Fichier | Contenu |
|---|---|
| `emprises.geojson` | emprises CLPA (WGS84, anneaux orientés selon la RFC 7946) qui touchent l'emprise du site ou sont dans la commune ; propriétés `ddt05_index`, `source`, `risque`, `information`, `moa`, `ref_saisie`, `insee`, `commune`, `area_m2` (Lambert-93) |
| `crossings.geojson` | tronçons de pistes et d'itinéraires dans une emprise (WGS84) ; propriétés `id`, `osm_way_id`, `name`, `relations`, `network` (`nordic`, `hike`), `length_m` (Lambert-93), `ddt05_indices`, `risque`, `alert_level` (`info`, `warning` au risque 3, `high` à 4-5), `alert` (texte en français) |
| `summary.json` | totaux par réseau et par piste, comparaison au rapport 07, BERA du jour si `bera/bera.json` existe, message |
| `avalanche-historic.meta.json` | sources, licences, attributions, date de génération |

Données :

- **Emprises** : couche `L_ALEA_AVALANCHE_005` de la DDT des Hautes-Alpes (« aléa avalanche », Licence Ouverte 2.0 sur data.gouv.fr), téléchargée par le téléchargeur commun (source `geoide`, cache 30 jours) depuis le service ATOM du ministère, lue par un petit lecteur de shapefile (`historic::shapefile`). Seuls les enregistrements dont la source contient `CLPA` sont des emprises d'avalanches historiques ; les zones de PPR ne servent qu'à la comparaison. La CLPA d'INRAE est réservée à un usage personnel ; le droit de la DDT à la republier en Licence Ouverte n'est pas vérifié (rapport 07).
- **Pistes** : la couche `pistes` du site produite par la crate `osm` (P2, `data/out/<site>/pistes/pistes.geojson`, à construire d'abord : `skimap pistes <site>`), lue par `pistes::from_pistes_geojson` ; sa source (extrait OpenSnowMap et date des données OSM) est reprise de `pistes.meta.json`. Un extrait Overpass enregistré par les recherches se lit avec `build_from_dump` pour refaire les chiffres du rapport 07. Le calcul ne prend que des `PisteLine` (`build_with_pistes`). Règle du rapport 07 : réseau nordique = ways dont `piste:type` contient `nordic` + membres des relations nordiques ; les ways `hike` restants forment le réseau `hike`. Une ligne appartient au site si la moyenne de ses sommets est dans la commune (contour geo.api.gouv.fr), ce qui exclut le versant d'Arvieux.
- **Calcul** : chaque segment est coupé à chaque croisement d'un bord d'emprise, chaque morceau est dedans ou dehors selon son milieu (règle pair-impair, trous respectés), les emprises qui se chevauchent comptent une fois. Longueurs euclidiennes en Lambert-93, comme le calcul de référence du rapport 07.
- **Jura** : pas de CLPA ; couche vide et message « l'absence de zone connue ne veut pas dire absence de danger ».

Jamais « sûr » : les zones CLPA sont des avalanches connues, une avalanche peut se produire ailleurs.

```sh
skimap pistes cervieres && skimap avalanche-historic cervieres                    # couche pistes (P2)
cargo run --release -p skimap-avalanche --example historic -- cervieres          # idem
cargo run --release -p skimap-avalanche --example historic -- cervieres research/raw/osm/cerv2.json
SKIMAP_RESEARCH_RAW=…/research/raw cargo test -p skimap-avalanche -- --ignored   # chiffres du rapport 07
```
