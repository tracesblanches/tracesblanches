# Modèle d'exécution et téléchargeur commun

[← Architecture](../architecture.md)

## Modèle d'exécution : bloquant

Tout le workspace est **bloquant** : `reqwest::blocking` pour le réseau, `rayon` ou `std::thread::scope` pour le parallélisme de calcul.

Raisons : le pipeline est un traitement par lots dont le débit réseau est borné par les quotas des sources, pas par la concurrence ; le calcul (relief, grilles, routage) se parallélise mieux avec rayon ; le noyau qui ira sur le téléphone n'a pas de réseau.

Aucune API publique d'une crate n'est `async`. Si une dépendance n'existe qu'en asynchrone, elle est encapsulée dans la crate avec un runtime local, sans exposer `async`.

TLS : rustls avec le fournisseur `ring` et les racines Mozilla (`webpki-roots`), configuré dans `common::download`. Pas d'aws-lc (compilation C) ni de vérificateur de la plateforme (il exige une JVM sur Android) : le même binaire fonctionne sur le poste et sur le téléphone.

## Téléchargeur commun (P1, #14)

`skimap_common::download::Downloader` :

- cache disque par source : `data/cache/<source>/<sha256>.body` et `.json` (URL publique, ETag, Last-Modified, dates, taille, sha256) ;
- requêtes conditionnelles `If-None-Match` / `If-Modified-Since`, réponse 304 servie depuis le cache ;
- politique de cache par requête : `Normal` (aucune requête tant que l'entrée a moins de `max_age`), `Revalidate`, `Reload`, `Offline` ;
- limites de débit par source en fenêtre glissante, persistées dans `data/cache/<source>/ratelimit.json` (un quota journalier survit à un redémarrage) ; une requête est inscrite avant l'envoi (un arrêt brutal ne l'efface pas du quota), puis datée de la fin de sa réponse : le serveur ne voit jamais deux requêtes plus proches que la fenêtre, quel que soit le temps passé entre l'inscription et l'envoi (écriture de l'état avec `fsync`, connexion, machine chargée) ; une attente au-delà de `max_wait` donne l'erreur `RateLimited` sans requête ;
- une seule requête en vol par source ; sources différentes en parallèle ;
- reprises avec attente exponentielle sur erreur réseau, 408, 429, 5xx, en respectant `Retry-After` ;
- User-Agent générique `skimap/<version> (offline nordic ski map data pipeline)`, sans donnée personnelle ;
- clés d'API passées par `secret_query` / `secret_header` : jamais dans la clé de cache, les métadonnées, les journaux ni les erreurs.

Tests (`crates/common/tests/download.rs`, serveur HTTP local) : aucune requête tant que le cache est frais, une seule requête pour quatre appels simultanés, 304 sur ETag et Last-Modified, espacement des requêtes, quota épuisé sans requête, état des quotas conservé entre deux instances, reprises bornées, `Retry-After`, secrets absents du cache et des erreurs.

## Quotas par défaut

Définis dans `download::known_sources`, modifiables par `SKIMAP_RATE_LIMIT_<SOURCE>=N/période` (ex. `1000/h`, `1/5s`, `100/15min,1000/d`).

| Source | Limite | Origine |
|---|---|---|
| `datatourisme` | 1/s et 1000/h | documentation API |
| `skitour` | 1/s et 1000/jour | ticket P1 |
| `overpass` | 1/5 s et 10 000/jour | consignes de l'instance publique |
| `meteofrance` | 50/min | annoncé, à confirmer |
| `ign` | 20/s | CGU : 400/s par IP pour le TMS |
| `geo-api` | 10/s | prudence |
| `opensnowmap` | 1/min | extrait quotidien |
| `open-meteo` | 1/s et 10 000/jour, cache 1 h | conditions d'Open-Meteo (rapport 03) |
| `openskimap` | 1/min et 2/jour, cache 1 jour | « at most once per day » par fichier (openskidata.org, rapport 07) |
| `skiradar` | 30/min | page data.gouv.fr du flux (rapport 03) |
| `ign-lidar-wms` | 20/s, cache 1 an | dalles LiDAR HD sans `ETag` ni `Last-Modified` ; prudence |
| `nordicfrance`, `cervieres-mairie` | 1/min, cache 3 h | prudence, non sourcé (HTML sans conditions) |
| `meteofrance-archive` | 1/s, cache 1 jour | prudence, non sourcé |
| `geoide` | 1/s, cache 30 jours | prudence, non sourcé (jeu statique) |
| `biodiv-sports`, `refuges-info`, `camptocamp`, `enj`, `datagouv`, toute autre source | 1/s | aucun quota publié, à confirmer |
