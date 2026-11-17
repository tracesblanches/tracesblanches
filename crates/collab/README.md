# skimap-collab

Collaboratif et sport : signalements de terrain (V8 #71 à V11 #74), corrections OSM (V12 #75), défis (V13 #76), import de l'export Strava (V15 #78). Le suivi de groupe en SRN (V14 #77) s'appuie sur le mode groupe (`crates/group`, `android/group`) et n'est pas ici.

- [Règles des signalements](docs/regles.md) : types, durées de vie, confirmation, réfutation, réputation, file hors réseau, modération.
- API du serveur et coût par utilisateur : [`crates/server/docs/collab.md`](../server/docs/collab.md).

| Module | Fonction | Côté |
|---|---|---|
| `rules` | types de signalement, durées de vie, évaluation (actif, retenu, expiré, réfuté), réputation, heure du constat acceptée | les deux |
| `jpeg` | photo : JPEG seulement, 60 Ko et 1280 px au plus, segments de métadonnées retirés (EXIF, XMP, ICC, commentaires) | serveur |
| `store` (fonction `server`) | SQLite : appareils (empreinte salée), signalements, votes, photos ; limites par appareil ; purge | serveur |
| `osm_note` | brouillon de note OSM (texte FR ou EN, piste, lien du chemin OSM, commentaire borné) | téléphone |
| `track`, `fit` (fonction `phone`) | GPX, TCX, FIT ; résumé `outing.json` (trace allégée à 10 m) ; entrée de la calibration C11 (`OutingObs`) | téléphone |
| `strava_export` (fonction `phone`) | archive d'export Strava lue sur le téléphone : `activities.csv`, fichiers d'origine, ski nordique et de randonnée seulement | téléphone |
| `challenges` (fonction `phone`) | couverture d'un itinéraire ou des pistes d'un domaine par les sorties, kilomètres de la saison | téléphone |

```sh
cargo test -p skimap-collab --all-features
cargo test --release -p skimap-collab --all-features --test real -- --ignored --nocapture   # données réelles
cargo run -p skimap-collab --features phone --example challenge_assets -- \
  data/out/france/srn/srn_routes.geojson android/community/src/main/assets/challenges.json
```

## Résultats (15/11/2026)

- Défi « domaine complet » sur les pistes des packs : Jeu de test alpin 196 lignes, 68,3 km ; les 12 itinéraires prêts du pack en couvrent 31,4 km (46 %). Jeu de test jurassien 691 lignes, 311,5 km (toutes les pistes de l'emprise du pack, Risoux et Mouthe compris) ; les 18 itinéraires en couvrent 60,7 km (19 %). Trois couvertures en 4 ms et 8 ms (release, poste).
- GTJ à ski de randonnée nordique (couche SRN, DATAtourisme) : 99,8 km (publiée : 100 km) ; 1374 points simplifiés à 789 (10 m), 15 Ko dans l'app.
- Stockage : 10 000 signalements insérés en 1,8 s, un site listé en 1 ms (SQLite en mémoire, release).

## Limites

- Couverture : un tronçon compte quand son milieu est à 40 m d'une trace ; deux pistes parallèles à moins de 40 m comptent ensemble.
- L'export Strava d'un compte en français peut avoir des en-têtes traduits : `ID de l'activité`, `Type d'activité`, `Nom du fichier` sont reconnus, d'autres variantes non (à vérifier sur une vraie archive française).
- FIT : seuls les messages `record` (position, heure, altitude) sont lus ; le CRC n'est pas vérifié.
