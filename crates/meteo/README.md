# skimap-meteo

Client des API Météo-France : BERA par massif et météo horaire (pipeline). Tickets : P8 (#21), P19 (#32).

## BERA par massif (P8, module `bera`)

`skimap_meteo::bera::build(site, &layout, &downloader, clé, date)` écrit `data/out/<site>/bera/` :

| Fichier | Contenu |
|---|---|
| `bera.json` | `BeraFile` : site, date, massif, statut (`bulletin`, `unavailable`, `no_bera`), origine (`api`, `archive`), URL du XML et du PDF, bulletin lu, message pour l'utilisateur, liens |
| `bera.xml` | bulletin XML brut, seulement quand un bulletin a été lu |
| `bera.meta.json` | sources (URL, licence, attribution), date de génération, validité (`DATEBULLETIN` → `DATEVALIDITE`), notes |

Sources, dans l'ordre :

1. **API « DonneesPubliquesBRA »** du portail Météo-France, avec la clé `METEOFRANCE_API_KEY` passée dans l'en-tête `apikey` (jamais journalisée ni mise en cache) : `GET https://public-api.meteofrance.fr/public/DPBRA/v1/massif/BRA?id-massif=17&format=xml`. Sans clé, le portail répond 401 (vérifié le 14/11/2026 sur `/massif/BRA` et `/liste-massifs` ; un chemin inconnu répond 404).
2. **Archive publique** `https://donneespubliques.meteofrance.fr/donnees_libres/Pdf/BRA/` : index du jour `bra.AAAAMMJJ.json`, puis `BRA.<MASSIF>.<horodatage>.pdf`. Le `.xml` y est encore servi jusqu'en avril 2023 ; depuis la saison 2023-24 il redirige vers une page « donnée indisponible » (reconnue et ignorée), seul le PDF reste. Le 14/11/2026, la saison en cours n'est pas encore dans l'archive (l'index du 01/11 est vide, les suivants redirigent).

Le lecteur XML suit la spécification « Descriptif technique des informations du bulletin avalanche » du 26/10/2023 et lit aussi l'ancien format de l'archive (racine `<Bulletins>`, neige fraîche par secteur `SS241`/`SS242`, images en base64, `TENDANCES`, `BSH/AVALANCHES`). Les valeurs sont gardées telles que publiées (`-1` = sans objet : pas de mer de nuages, temps sec, vent absent).

Massifs : table des 36 massifs de l'annexe 1 (`MASSIFS`), avec le nom utilisé par l'archive. Cervières = Queyras (17).

Jura : pas de BERA (absent de l'annexe 1). Chapelle-des-Bois reçoit un `bera.json` au statut `no_bera`, avec un message et des liens : météo montagne Jura et vigilance du Doubs de Météo-France, bulletin du SLF qui couvre le Jura suisse voisin (régions CH-8111 Saint-Cergue, CH-8112 Vallée de Joux, CH-8211 à 8213, vues dans l'API CAAML du SLF pour le 15/02/2026).

Le message affiché ne remplace jamais le bulletin officiel : il rappelle de lire le BERA complet.

```sh
cargo run -p skimap-meteo --example bera -- cervieres 2023-04-15   # archive, XML réel lu
cargo run -p skimap-meteo --example bera -- cervieres               # aujourd'hui (API si clé)
cargo run -p skimap-meteo --example bera -- chapelle-des-bois       # notice Jura
```

Tests : exemple Belledonne de la spécification recopié (`tests/fixtures/bera_spec_belledonne_20231026.xml`) et bulletin réel du Queyras du 15/04/2023 tiré de l'archive publique, sans le contenu base64 des images (`tests/fixtures/BRA.QUEYRAS.20230415135925.noimg.xml`).
