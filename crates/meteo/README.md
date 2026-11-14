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

## Météo heure par heure par site (P19, module `hourly`)

`skimap_meteo::hourly::build(site, &layout, &downloader, personal_sources)` écrit `data/out/<site>/meteo-hourly/` :

| Fichier | Contenu |
|---|---|
| `hourly.json` | `HourlyFile` : par point de prévision, altitude du modèle, modèle de chaque variable, 72 heures (heure locale) et un résumé par jour |
| `meteo-hourly.meta.json` | source, licence, attribution, période couverte, notes |

Variables par heure : température à 2 m, température apparente (Open-Meteo), indice de refroidissement éolien calculé (formule JAG-TI d'Environnement Canada, la « température ressentie » de Météo-France ; défini sous 10 °C et au-delà de 4,8 km/h), vent moyen, rafales, direction, visibilité, brouillard (visibilité < 1000 m ou code OMM 45/48), isotherme 0 °C, chute de neige, précipitations, nébulosité totale et basse, code OMM. Par jour : neige cumulée, nébulosité moyenne de 9 h à 16 h (indicateur de soleil), rafale maximale, refroidissement minimal, heures de brouillard de 9 h à 16 h, isotherme 0 °C le plus bas.

Points de prévision : Cervières, Le Laus (1750 m) et Les Fonts (2044 m), d'après les nœuds OSM `place=hamlet` ; Chapelle-des-Bois, départ des pistes (1087 m), premier point des itinéraires ENJ. Le centre de la commune de Cervières (geo.api.gouv.fr) est à 2431 m : il ne représente pas le domaine.

Source actuelle : **Open-Meteo**, gratuit pour un usage non commercial seulement (données CC BY 4.0), donc derrière `personal_sources` : `build` refuse de tourner sans l'option (erreur `PersonalSourcesDisabled`, rien n'est écrit). Deux appels par point, cache d'une heure :

- `models=meteofrance_seamless` (AROME et ARPEGE) pour la température, le vent, la neige, les nuages et le code OMM ;
- `models=best_match` pour `visibility` et `freezing_level_height`, nuls dans les modèles Météo-France d'Open-Meteo (vérifié le 14/11/2026 à Cervières).

`snow_depth` n'est jamais lu : nul avec `meteofrance_seamless` (rapport 03) ; la hauteur de neige vient de la couche d'enneigement (P6).

### Passage à AROME de Météo-France

Deux voies, non codées :

1. **API « AROME » du portail Météo-France**, avec la clé `METEOFRANCE_API_KEY` dans l'en-tête `apikey` : service WCS 2.0.1 `https://public-api.meteofrance.fr/public/arome/1.0/wcs/MF-NWP-HIGHRES-AROME-001-FRANCE-WCS/` (`GetCapabilities` répond 401 sans clé le 14/11/2026), puis `GetCoverage` par paramètre avec un sous-ensemble lat/long/temps autour de chaque point. Petites extractions ; quotas du portail à confirmer.
2. **« Paquets AROME 0,01° » en données ouvertes** (Licence Ouverte, sans clé) sur data.gouv.fr : fichiers GRIB2 horaires de toute la grille EURW1S100 (mis à jour en continu). Il faut un décodeur GRIB2 (absent du workspace) et de gros téléchargements.

Dans les deux cas, seule la fonction qui remplit `Hour` change ; `hourly.json` garde le même format, avec le modèle de chaque variable dans `models`.

```sh
SKIMAP_PERSONAL_SOURCES=1 cargo run -p skimap-meteo --example hourly -- cervieres
SKIMAP_PERSONAL_SOURCES=1 cargo run -p skimap-meteo --example hourly -- chapelle-des-bois
```

`SKIMAP_PERSONAL_SOURCES=1` (ou `true`) active `skimap_common::config::Config::personal_sources`.
