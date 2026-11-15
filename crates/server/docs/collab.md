# Signalements collaboratifs (V8 #71)

[← skimap-server](../README.md)

Routes sous `/v1/collab/` (`src/collab.rs`), règles et stockage dans `crates/collab` ([règles](../../collab/docs/regles.md)). Base SQLite à part : `SKIMAP_COLLAB_DB` (défaut `/data/server/collab.sqlite`), purgée toutes les heures.

## Points d'accès

| Méthode, chemin | Corps | Réponse |
|---|---|---|
| `GET /v1/collab/rules` | — | types, durées de vie, limites de la photo, `free_text: false` |
| `GET /v1/collab/reports?bbox=minLon,minLat,maxLon,maxLat` (2° au plus de côté) | — | `{now, reports: [...]}` actifs, 500 au plus, plus récents d'abord ; `Cache-Control: public, max-age=60` (une requête par domaine et par minute atteint l'origine, quel que soit le nombre de téléphones) |
| `POST /v1/collab/reports` | `{kind, lon, lat, seen_at, device}` | 201 et le signalement ; 400 (type, position, heure, identifiant), 410 (déjà expiré), 429 |
| `GET /v1/collab/reports/<id>` | — | le signalement, quel que soit son état ; 404 une fois purgé |
| `POST /v1/collab/reports/<id>/confirm`, `…/deny` | en-tête `X-Skimap-Device` | 200 et le signalement ; 403 sur le sien ; 404 expiré ou réfuté |
| `POST /v1/collab/reports/<id>/photo` | JPEG brut, en-tête `X-Skimap-Device` (auteur) | 204 ; 400 pas un JPEG ou trop grand en pixels ; 409 déjà une photo ; 413 plus de 64 Ko |
| `POST /v1/collab/reports/<id>/photo/flag` | en-tête `X-Skimap-Device` | 204 : photo supprimée |
| `DELETE /v1/collab/reports/<id>` | en-tête `X-Skimap-Device` (auteur) | 204 |
| `GET /v1/collab/photos/<id>.jpg` | — | la photo si montrée, sinon 404 |

Un signalement : `id`, `kind`, `lon`, `lat`, `seen_at`, `expires_at`, `state` (`active`, `held`, `expired`, `refuted`), `confirmations`, `denials`, `photo`.

## Données personnelles (RGPD)

- Pas de compte. L'app tire un identifiant aléatoire de 128 bits, le renouvelle tous les 30 jours ; le serveur n'en garde que l'empreinte SHA-256 salée (sel aléatoire propre à la base), jamais l'identifiant (test : l'identifiant n'apparaît nulle part dans le fichier SQLite ni dans son journal).
- Pas d'adresse IP stockée : la limite par adresse est en mémoire et les journaux du serveur ne gardent ni adresse ni chaîne de requête.
- Pas de texte libre. Photo sans métadonnées (EXIF : position, appareil, heure), supprimée avec son signalement (24 h à 7 jours, 28 jours au plus avec confirmations).
- Effacement à la demande : `DELETE` par l'auteur ; les appareils non revus depuis 60 jours sont oubliés.
- La page de confidentialité (`src/privacy.rs`) doit mentionner ces données : demandé à l'orchestrateur (fichier hors de ce ticket).

## Coût par utilisateur (critère de V8)

Hypothèse haute : 1 000 utilisateurs actifs un jour de pointe, 5 signalements chacun (dont 1 sur 5 avec photo de 40 Ko), 20 votes, 30 ouvertures de la carte.

| Poste | Calcul | Total |
|---|---|---|
| Stockage | 5 000 lignes × ~300 o + 1 000 photos × 40 Ko, durée de vie ≤ 7 jours (28 avec confirmations) | ≈ 40 Mo par jour de pointe, < 300 Mo au pire sur le volume existant |
| Requêtes d'écriture à l'origine | 25 000 par jour, ~0,2 ms chacune (mesure : 10 000 insertions en 1,8 s) | < 10 s de processeur par jour |
| Lectures | mises en cache 60 s par Cloudflare par domaine : au plus 1 440 par domaine et par jour à l'origine, 1 ms chacune | négligeable |
| Trafic sortant | listes de quelques Ko, photos servies par Cloudflare | dans l'offre gratuite |

Le service tourne dans le pod existant (k3s, offre gratuite) : coût marginal **0 € par utilisateur et par an**, sans brique facturée à l'usage. Plafonds qui le garantissent : 20 000 signalements par jour pour tout le service, limites par appareil et par adresse.

## Vérifier

```sh
cargo test -p skimap-collab --all-features      # règles, stockage en mémoire, horloge simulée
cargo test -p skimap-server --test collab        # HTTP, file hors réseau, expiration, photo, limites
```
