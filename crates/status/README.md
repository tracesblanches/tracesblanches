# skimap-status

Instantané de l'état des pistes (pipeline), interrogé une fois pour tous les utilisateurs. Ticket : P13 (#26).

`skimap_status::poll(site, &layout, &downloader, personal_sources)` interroge les sources du site, enregistre un instantané horodaté et écrit `data/out/<site>/status/` :

| Fichier | Contenu |
|---|---|
| `status.json` | dernier contenu normalisé de chaque source du site |
| `status.geojson` | (ENJ) itinéraires en WGS84 avec `state` (`open`, `closed`), `route`, `notes`, `update_datetime`, `practice`, `difficulty`, `length_m`, `gtj`, lien `gpx`, `polled_at` |
| `<source>/polls.jsonl` | une ligne par interrogation : heure, origine (`downloaded`, `not_modified`, `cache`), changement ou non, instantané |
| `<source>/snapshots/AAAAMMJJTHHMMSSZ.json` | contenu normalisé, écrit seulement quand il change |
| `<source>/history.json` | fréquence observée : nombre d'interrogations, durée couverte, contenus distincts, heures des changements, intervalle moyen, dates publiées par la source et leur première observation ; pour l'ENJ, transitions d'état par itinéraire et heures de la journée des notes d'état |
| `status.meta.json` | sources, licences, attributions |

## Sources

| Site | Source | Accès | Fréquence |
|---|---|---|---|
| Chapelle-des-Bois | API Geotrek de l'ENJ, `trek/?cities=14&language=fr&fields=…` (champ `route` : 1 Fermé, 2 Ouvert ; `labels` « Marquée … le JJ/MM/AAAA à HHhMM » ; `update_datetime`), géométries une fois par jour | sans clé ; ni `ETag` ni `Last-Modified`, donc la liste (9 ko avec `fields=`) est téléchargée à chaque fois | toutes les 15 min (durée de cache de la source `enj`) |
| Cervières | widget Nordic France `/widget/marqueblanche/station/3167` : secteurs, date de damage par secteur, état et commentaire par piste, « Mise à jour » | HTML seulement | au plus toutes les 3 h |
| Cervières | page « Info damage » de la mairie : un tableau par secteur (piste, ouvert/fermé, damage, classique, skating, commentaire), `article:modified_time` | HTML seulement | au plus toutes les 3 h |

**Usage personnel** : aucune de ces sources n'affiche de licence (ENJ : ticket L1 ouvert ; Nordic France et mairie : HTML sans conditions). `poll` refuse donc de tourner sans `personal_sources` (erreur `PersonalSourcesDisabled`, rien n'est écrit). L'option n'existe pas encore dans `skimap_common::config::Config` ; l'exemple la lit dans `SKIMAP_PERSONAL_SOURCES=1`.

## Planification

Le pipeline appelle `poll` à intervalle fixe, par exemple toutes les 15 minutes (cron ou minuterie systemd) :

```sh
*/15 * * * * cd /chemin/skimap && SKIMAP_PERSONAL_SOURCES=1 target/release/examples/poll chapelle-des-bois
0 */3 * * *  cd /chemin/skimap && SKIMAP_PERSONAL_SOURCES=1 target/release/examples/poll cervieres
```

Une semaine de ces instantanés en saison donne la fréquence réelle de mise à jour de chaque source (`history.json`).
