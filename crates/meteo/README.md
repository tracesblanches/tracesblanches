# skimap-meteo

Client des API Météo-France : BERA par massif et météo horaire (pipeline). Tickets : P8 (#21), P19 (#32), X17 (#151).

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

Jours passés : `skimap_meteo::hourly::history(site, &downloader, personal_sources, premier, dernier, dossier)` (API « historical forecast » d'Open-Meteo, mêmes modèles et variables, à partir de trois jours avant la première date) écrit `meteo_<début>_<fin>.json` au même format ; `skimap history <site> <premier> <dernier>` le range dans `data/out/<site>/assist/history/` avec les cartes d'enneigement et les BERA des mêmes jours (remplace l'exemple `history` de l'assistant ; identique à sa sortie sur Cervières du 01/01 au 10/01/2026 : 480 heures, aucune valeur différente).

`skimap_meteo::hourly::build(site, &layout, &downloader, personal_sources)` écrit `data/out/<site>/meteo-hourly/` :

| Fichier | Contenu |
|---|---|
| `hourly.json` | `HourlyFile` : par point de prévision, altitude du modèle, modèle de chaque variable, 72 heures (heure locale) et un résumé par jour |
| `meteo-hourly.meta.json` | source, licence, attribution, période couverte, notes |

Variables par heure : température à 2 m, humidité relative à 2 m (`relative_humidity_pct`, fartage A2), température apparente (Open-Meteo), indice de refroidissement éolien calculé (formule JAG-TI d'Environnement Canada, la « température ressentie » de Météo-France ; défini sous 10 °C et au-delà de 4,8 km/h), vent moyen, rafales, direction, visibilité, brouillard (visibilité < 1000 m ou code OMM 45/48), isotherme 0 °C, chute de neige, précipitations, nébulosité totale et basse, code OMM. Par jour : neige cumulée, nébulosité moyenne de 9 h à 16 h (indicateur de soleil), rafale maximale, refroidissement minimal, heures de brouillard de 9 h à 16 h, isotherme 0 °C le plus bas.

Points de prévision : Cervières, Le Laus (1750 m) et Les Fonts (2044 m), d'après les nœuds OSM `place=hamlet` ; Chapelle-des-Bois, départ des pistes (1087 m), premier point des itinéraires ENJ. Le centre de la commune de Cervières (geo.api.gouv.fr) est à 2431 m : il ne représente pas le domaine.

Source actuelle : **Open-Meteo**, gratuit pour un usage non commercial seulement (données CC BY 4.0), donc derrière `personal_sources` : `build` refuse de tourner sans l'option (erreur `PersonalSourcesDisabled`, rien n'est écrit). Deux appels par point, cache d'une heure :

- `models=meteofrance_seamless` (AROME et ARPEGE) pour la température, le vent, la neige, les nuages et le code OMM ;
- `models=best_match` pour `visibility` et `freezing_level_height`, nuls dans les modèles Météo-France d'Open-Meteo (vérifié le 14/11/2026 à Cervières).

`snow_depth` n'est jamais lu : nul avec `meteofrance_seamless` (rapport 03) ; la hauteur de neige vient de la couche d'enneigement (P6).

### Source publiable : AROME de Météo-France en données ouvertes (X17, #151, module `arome`)

`skimap_meteo::arome::build(site, &layout, &downloader)` écrit la même couche `meteo-hourly/` (même `HourlyFile`, l'assistant la lit telle quelle) sans `personal_sources` ; `skimap_meteo::arome::refresh(&[sites], &layout, &downloader)` est l'entrée du serveur.

- **Source** : « Paquets AROME - Résolution 0,025° » de Météo-France sur data.gouv.fr, **Licence Ouverte 2.0**, sans clé, sans quota publié. Fichiers servis par le dépôt public `https://meteofrance-pnt.s3.rbx.io.cloud.ovh.net/pnt/<run>/arome/0025/<paquet>/` (les URL des ressources du jeu de données) ; la liste S3 (`?list-type=2&prefix=pnt/&delimiter=/`) donne les runs (deux jours gardés). Huit runs par jour (00 à 21 UTC), échéances 0 à 51 h, GRIB2 compressé CCSDS (gabarit 5.42), grille EURW1S40 avec valeurs manquantes hors du domaine trapézoïdal (bitmap). L'API WCS du portail (clé `METEOFRANCE_API_KEY`) n'est pas nécessaire.
- **Paquets lus** : `SP1` (9 fichiers par run, 470 Mo) : T(2m), HU(2m), FF et DD(10m), FF_RAF(10m) (maximum sur l'heure), NEBUL, PRECIP, NEIGE, GRAUPEL (cumulés depuis le début du run) ; l'altitude du point de grille vient de `SP2` 00-06 h (48 Mo) : le fichier des champs constants est en compression 5.50002, non lue par le décodeur. Plus le fichier `SP1` 00-06 h des cinq runs précédents (toutes les 6 h, 58 Mo chacun) : leurs heures 1 à 6 donnent les 24 h d'historique que demandent la qualité de neige et le fartage (série de 81 heures, de la veille au surlendemain).
- **Décodage** : crate `grib` 0.20.3 avec `rust-aec` (Rust pur, sans libaec), un fil par fichier, chaque champ décodé une fois pour tous les points de tous les sites ; point de grille le plus proche (0,025°).
- **Absent des paquets de surface** (valeurs nulles) : visibilité et brouillard, isotherme 0 °C (seulement en niveaux isobares ou hauteurs : 444 à 700 Mo par groupe d'échéances), code temps OMM, nébulosité basse (`SP2` entier, 375 Mo, non lue par l'assistant). Température apparente calculée (Steadman 1994, BoM, sans rayonnement), refroidissement éolien comme P19.
- **Unités** : neige en cm = (NEIGE + GRAUPEL) en mm d'eau × 0,7 (la conversion d'Open-Meteo, pour garder l'unité que lit l'assistant) ; précipitations et neige horaires = différences des cumuls.
- **Méta** : `meteo-hourly.meta.json` marque la source publiable (`personal_use_only` faux), licence, attribution « Météo-France, modèle AROME 0,025° (Licence Ouverte 2.0) », run lu (`parameters.arome_run`, `past_runs`), point de grille de chaque point (`parameters.points`).
- **Idempotence** : `refresh` ne réécrit un site que si sa couche ne contient pas déjà le run le plus récent (`parameters.arome_run`) ; un second appel dans le même run n'écrit rien et ne télécharge rien.

Validation du décodage (oracle Python `eccodes` hors dépôt, run du 15/11/2026 09 UTC) : tous les champs utiles du fichier `SP1` 00-06 h (58 champs) à cinq points (Le Laus, Chapelle-des-Bois, Plateau de Beille, Vercors, un point hors domaine en mer Baltique) et l'altitude de `SP2` à trois points : même indice de grille, écart maximal 1,2·10⁻⁵ (précision f32), valeurs manquantes identiques.

Limites : horizon 51 h (le jour J+2 n'est couvert que jusqu'à la fin du run) ; pas de correction d'altitude (Le Laus : point de grille à 2027 m pour un départ à 1750 m ; Les Fonts : 2401 m pour 2044 m) ; heures du jour en cours avant le run absentes si les runs précédents ne sont plus publiés.

Intégration au rattrapage quotidien du serveur : appeler `arome::refresh(&sites, &layout, &dl)` avec tous les sites à servir, une fois par run (toutes les 3 h) ou au moins chaque matin après le run de 03 UTC ; sans effet si le run n'a pas changé. Le cache du téléchargeur garde les fichiers d'un run (environ 810 Mo par rafraîchissement, dont 290 Mo réutilisés au run suivant) : prévoir une purge des entrées `meteofrance-pnt` de plus de deux jours.

```sh
cargo run --release -p skimap-meteo --example arome -- cervieres chapelle-des-bois plateau-de-beille
cargo run --release -p skimap-meteo --example arome -- --file <fichier.grib2> 44.8552 6.7269
```

### Open-Meteo (usage personnel)

```sh
SKIMAP_PERSONAL_SOURCES=1 cargo run -p skimap-meteo --example hourly -- cervieres
SKIMAP_PERSONAL_SOURCES=1 cargo run -p skimap-meteo --example hourly -- chapelle-des-bois
```

`SKIMAP_PERSONAL_SOURCES=1` (ou `true`) active `skimap_common::config::Config::personal_sources`.
