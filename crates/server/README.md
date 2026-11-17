# skimap-server

Serveur de skimap sur le cluster k3s (arm64) : il **distribue** les packs et leurs mises à jour. Le téléphone fait tout hors ligne et fonctionne entièrement sans lui ; ce qui est critique pour la sécurité (alerte de non-retour par SMS, K10 #61) est sur le téléphone. Tickets : S0 (#50), S1 (#62), S2 (#60, complément facultatif), page de confidentialité de K14 (#122), précalculs côté serveur E1 (#125).

## Principes

- **Aucun calcul par requête** : à chaque publication (par domaine, quel que soit le nombre de téléphones), `skimap-server publish` écrit des fichiers statiques : `catalog.json`, `sites/<site>/index.json` et des paquets immuables nommés par leur empreinte (`bundles/<sha256>.skb`). Le serveur ne fait que les servir (`ETag`, `If-None-Match`, `Range`), derrière le cache de Cloudflare.
- **Une seule requête minuscule pour une synchro sans changement** (304), deux pour une synchro quotidienne (l'index et un paquet de quelques centaines de ko). Formats et mesures : [`crates/pack/docs/mise-a-jour.md`](../pack/docs/mise-a-jour.md).
- **Les sources sont interrogées par le serveur**, une fois par domaine (ou par massif : le cache de téléchargement commun sert le second site d'un massif sans nouvelle requête) et par période, par les CronJobs du pipeline ; jamais par les téléphones.
- **États par utilisateur** : seulement l'alerte de non-retour facultative (S2, effacée 7 jours après la sortie) ; l'échange Strava (S1) ne garde rien.
- **Bloquant**, comme tout le workspace : tiny_http (déjà au workspace) avec un groupe fixe de 16 fils, SQLite (rusqlite), reqwest bloquant pour Strava, un petit client SMTP sur rustls. Raisons : le trafic d'origine est faible (les paquets viennent du cache Cloudflare), un groupe fixe borne la mémoire (mesuré : 7,3 Mo au repos, 10,7 Mo après 20 téléchargements parallèles de 8,5 Mo), aucune dépendance async à ajouter. Limite : un téléchargement lent occupe un fil pendant sa durée.

## Points d'accès

| Méthode, chemin | Rôle |
|---|---|
| `GET /healthz` | 200 si le dépôt est lisible, si SQLite répond et si la minuterie des alertes a tourné il y a moins de 2 min ; sinon 503 et la sonde de vie redémarre le pod |
| `GET /privacy` (`/confidentialite`) | politique de confidentialité, français puis anglais (Play Store, K14) |
| `GET /v1/catalog.json` | catalogue des sites |
| `GET /v1/sites/<site>/index.json` | dernière version, paquet de base, paquets différentiels, changements et validité des couches |
| `GET /v1/bundles/<sha256>.skb` | paquet, immuable, requêtes partielles |
| `POST /v1/trips`, `GET /v1/trips/<id>`, `POST …/checkin`, `…/extend`, `…/position`, `…/cancel` (ou `DELETE`) | alerte de non-retour (S2), jeton `Bearer` rendu à la création |
| `GET /v1/strava/config`, `GET /v1/strava/callback`, `POST /v1/strava/token`, `…/refresh`, `…/revoke` | échange OAuth Strava (S1) |
| `GET /v1/collab/reports?bbox=…`, `POST /v1/collab/reports`, `…/<id>/confirm`, `…/deny`, `…/photo` | signalements collaboratifs (V8 à V11) : [docs/collab.md](docs/collab.md), base `SKIMAP_COLLAB_DB` (défaut : `collab.sqlite` à côté de `SKIMAP_SERVER_DB`) |

Référence pour l'app (K2) : [docs/api.md](docs/api.md). Alerte : [docs/alerte.md](docs/alerte.md). Strava : [docs/strava.md](docs/strava.md). Coûts : [docs/couts.md](docs/couts.md). Déploiement et lieux de calcul : [`deploy/README.md`](../../deploy/README.md).

## Commandes

```sh
skimap-server serve                              # variables ci-dessous
skimap-server publish data/out/<site>/pack    # JSON : paquets écrits, delta quotidien, base, ménage
skimap-server publish-france data/out/france-basemap   # fond de France (X20) : JSON, fichiers écrits, ménage
skimap-server send-test-email <adresse>
cargo test -p skimap-server                      # dont faux relais SMTP, faux Strava, horloge simulée
cargo test -p skimap-server --test smtp -- --ignored   # STARTTLS réel avec smtp.gmail.com:587
cargo run --release -p skimap-server --example phone_sync -- http://127.0.0.1:8080 <site> /tmp/phone/<site>
```

## Configuration (variables d'environnement)

| Variable | Défaut | Rôle |
|---|---|---|
| `SKIMAP_SERVER_ADDR`, `SKIMAP_SERVER_WORKERS` | `0.0.0.0:8080`, 16 | écoute |
| `SKIMAP_STORE` | `/data/store` | dépôt publié (`public/` servi ; `blobs/` et `sites/` internes) |
| `SKIMAP_SERVER_DB` | `/data/server/skimap.sqlite` | alertes |
| `SKIMAP_PUBLIC_URL` | `https://skimap.warnier.net` | cité dans les messages |
| `SKIMAP_CONTACT_EMAIL` (obligatoire pour `serve`), `SKIMAP_CONTROLLER` | — | contact et responsable cités par la page de confidentialité (fixés dans le manifeste) |
| `SKIMAP_RECENT_VERSIONS` | 8 | versions gardées en plus de la base (départs des paquets différentiels) |
| `SMTP_HOST`, `SMTP_PORT`, `SMTP_TLS` (`starttls`, `implicit`, `none` sur la boucle locale seulement), `SMTP_USER`, `SMTP_PASS`, `MAIL_FROM`, `MAIL_FROM_NAME` | port 587, STARTTLS | relais d'envoi des alertes ; sans `SMTP_HOST`, l'alerte est désactivée : `/v1/trips` répond 503 (journal : `alerts disabled`) |
| `SKIMAP_ALERT_DAILY_CAP` | 400 | plafond d'envois sur 24 h (sous les 500 destinataires par jour de Gmail) |
| `STRAVA_CLIENT_ID`, `STRAVA_CLIENT_SECRET`, `STRAVA_SCOPE`, `STRAVA_APP_REDIRECT`, `STRAVA_BASE_URL` | `activity:write`, `skimap://strava/callback`, `https://www.strava.com` | échange Strava ; sans identifiants : 503 |

Les secrets ne sont jamais journalisés (`Debug` masqué) ; les journaux ne gardent ni adresse IP ni chaîne de requête (le code OAuth Strava passe dans la chaîne du rappel).

## Publication

`publish` vérifie le pack (`Pack::verify`), ne republie pas un pack identique au dernier à `pack.meta.json` près (date de construction), copie les nouveaux contenus, choisit la base (reconstruite quand le paquet depuis elle dépasse 10 % du pack), écrit les paquets depuis la base et depuis les 8 dernières versions, puis l'index et le catalogue (renommages atomiques : un index ne nomme jamais un paquet absent), et supprime les contenus et paquets que plus rien ne nomme (ceux de l'index précédent restent une publication de plus, pour les téléchargements en cours). Mesuré le 15/11/2026 : première publication 5,7 s (jeu de test alpin) et 2,5 s (jeu de test jurassien) ; publication quotidienne 0,17 s et 0,13 s. Chaque relance d'une couche réécrit sa date de génération dans ses métadonnées et produit donc une nouvelle version (123 ko sur le jeu de test jurassien) : un CronJob par mise à jour des données, pas plus.

Sans `personal_sources` (le cas du serveur), le pack ne contient pas `status`, `status-summary` (état des pistes : ENJ, pages « Info damage » des mairies), `meteo-hourly` (Open-Meteo, usage non commercial), ni `assist-briefing`, `assist-wax`, `assist-snow-quality`, `assist-suggestions` (l'assistant lit la météo et l'état des pistes) ; le recensement des domaines n'utilise pas Nordic France. Le pack du jeu de test alpin passe de 45,1 à 43,9 Mo.

## Limites

- Images arm64 testées sous QEMU sur le poste, pas encore sur le serveur : la signature SSH (1Password) était indisponible le 15/11/2026, donc ni buildkitd du cluster, ni registre, ni `kubectl`.
- Cloudflare : `browser_check` est actif sur toute la zone ; l'app doit envoyer un `User-Agent` (OkHttp le fait). Une règle de configuration par hôte le désactiverait pour `skimap.warnier.net` (les 5 règles WAF gratuites sont prises).
- Gmail : 500 destinataires par jour pour un compte gratuit, d'où le plafond de 400.
