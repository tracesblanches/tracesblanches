# Déploiement

Ce dossier contient les images du serveur et du pipeline, le script des tâches planifiées et des exemples de CronJobs Kubernetes. Les manifestes du déploiement en service (volumes, secrets, DNS, cache) restent dans un dépôt d'infrastructure privé : ils ne sont pas nécessaires pour construire ni pour essayer le projet.

## Images (`deploy/Dockerfile`)

| Cible | Contenu | Base |
|---|---|---|
| `server` | `skimap-server` | `gcr.io/distroless/cc-debian13:nonroot` (glibc, pas de shell) |
| `pipeline` | `skimap`, `skimap-server` (publication), `skimap-job` | `debian:trixie-slim` (sh, flock) |

Une seule étape de compilation (`cargo build --release --locked`) sert aux deux cibles. Elle compile nativement sur une machine arm64, ou en croisé depuis amd64 (gcc-aarch64-linux-gnu) sans émulation. Les conteneurs tournent sous l'utilisateur 65532, système de fichiers en lecture seule.

```sh
docker buildx build --platform linux/arm64 -f deploy/Dockerfile --target server   -t skimap-server .
docker buildx build --platform linux/arm64 -f deploy/Dockerfile --target pipeline -t skimap-pipeline .
```

## Tâches planifiées (`deploy/skimap-job`)

`skimap-job <tâche> [<site>...]` lance une étape du pipeline puis publie les packs touchés. Les étapes attendent leur tour sur un verrou du volume (`flock`) ; une étape en échec garde les sorties précédentes de sa couche.

| Tâche | Ce qu'elle fait |
|---|---|
| `build-all` | toutes les étapes d'un site (lourd) |
| `snow` | neige de la veille, rasters du routeur |
| `bera` | bulletin d'avalanche des sites dont le massif a un BERA |
| `status` | état des pistes (sources à usage personnel : désactivé par défaut) |
| `refresh` | fiches des stations et refuges, puis les packs dont le contenu a changé |
| `catchup` | météo AROME du dernier run, puis seulement les étapes en échec, altérées ou anciennes (`build-all --stale`) ; chaque site est publié dès qu'il est construit |
| `france-basemap` | fond de carte de France (zooms 0 à 6), renouvelé au plus une fois par semaine |

Exemples de CronJobs : `catchup-cronjob.yaml`, `refresh-cronjobs.yaml`, `france-basemap-cronjob.yaml`. Ils supposent une image `skimap-pipeline`, un volume partagé et, pour le BERA, un secret qui fournit `METEOFRANCE_API_KEY`.

Les sources à usage personnel (`SKIMAP_PERSONAL_SOURCES=1`) ne doivent jamais tourner sur un serveur qui publie : leurs couches partiraient vers tous les téléphones.

## Serveur

Configuration de `skimap-server` (variables d'environnement, alertes, Strava) : [crates/server/README.md](../crates/server/README.md). API : [crates/server/docs/api.md](../crates/server/docs/api.md).
