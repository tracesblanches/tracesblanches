# API du serveur pour l'app (K2)

[← skimap-server](../README.md)

URL de base : `https://skimap.warnier.net`. Toutes les URL des documents sont relatives à cette base. Réponses d'erreur : `{"error": "<code>", "message": "..."}` avec le statut HTTP. L'app envoie un `User-Agent` (contrôle de navigateur de Cloudflare).

**Rien de critique ne dépend du serveur** : sans réseau ou si le serveur est injoignable, le pack installé reste complet (cartes, itinéraires, temps estimés, voix) et chaque couche affiche sa date de validité ; l'alerte de non-retour principale passe par SMS depuis le téléphone (K10, #61).

## Packs : synchronisation

Détail des formats, du plan et des mesures : [`crates/pack/docs/mise-a-jour.md`](../../pack/docs/mise-a-jour.md). En résumé :

1. `GET /v1/catalog.json` (liste des zones, à l'ouverture de l'écran de choix) ; `If-None-Match` → 304.
2. `GET /v1/sites/<site>/index.json` avec `If-None-Match` : 304 si rien de neuf (une requête, corps vide).
3. Les paquets de `skimap_pack::sync::plan(index, état)` : `GET /v1/bundles/<sha256>.skb`, reprise avec `Range: bytes=<déjà reçu>-` et `If-Range: "<sha256>"` (l'`ETag` d'un paquet est son empreinte) ; installation par `skimap_pack::update::apply_bundle`.

| Ressource | `Cache-Control` | `ETag` |
|---|---|---|
| `catalog.json`, `index.json` | `public, max-age=60, must-revalidate` | taille et date du fichier |
| `bundles/<sha256>.skb` | `public, max-age=31536000, immutable` | `"<sha256>"` |

Requêtes partielles : une seule plage par requête (206, `Content-Range`) ; plage hors du fichier : 416 ; plusieurs plages : fichier entier (200). Sur un défaut de cache, Cloudflare peut répondre 200 avec le fichier entier à une requête partielle : l'app doit accepter un 200 et repartir de zéro.

## Alerte de non-retour (S2, facultative)

Complément de l'alerte par SMS du téléphone (K10) pour le seul cas où le téléphone ne peut plus rien envoyer. Choix du canal, coût, limites : [alerte.md](alerte.md).

`POST /v1/trips` (JSON, 64 Kio au plus ; 10 par heure et par adresse, 500 par jour en tout) :

```json
{
  "skier_name": "Marie Dupont",
  "skier_phone": "+33 6 12 34 56 78",
  "contacts": ["contact1@example.org", "contact2@example.org"],
  "deadline": "2027-01-10T16:00:00Z",
  "grace_minutes": 30,
  "site": "<site>",
  "plan": "Boucle par le Bourget et le col des Thures, retour par la piste des Fonts.",
  "route": [[6.725, 44.8589], [6.76, 44.855], [6.8157, 44.8447]],
  "position": {"lon": 6.7251, "lat": 44.859, "accuracy_m": 8}
}
```

Contraintes : 1 à 3 adresses ; échéance entre 5 minutes et 7 jours ; délai de grâce 0 à 180 minutes (30 par défaut) ; nom 80 caractères, plan 2 000, itinéraire 2 000 points `[lon, lat]` ; `site` parmi les sites connus.

Réponse 201 : `{"id", "token", "trip"}`. Le jeton (64 caractères hexadécimaux) n'est montré qu'une fois et le serveur n'en garde que l'empreinte ; tous les appels suivants portent `Authorization: Bearer <token>` (sinon 404).

| Appel | Effet |
|---|---|
| `GET /v1/trips/<id>` | état : `active`, `alerted`, `returned`, `cancelled` ; `alert_at` ; dernière position ; `notifications` (`alert` ou `returned`, destinataire, `pending`, `sent`, `failed`, `cancelled`, tentatives) |
| `POST /v1/trips/<id>/position` `{"lon", "lat", "at"?, "accuracy_m"?}` | dernière position connue (quand le réseau passe) |
| `POST /v1/trips/<id>/extend` `{"deadline"}` | repousse l'échéance (avant l'alerte seulement ; 409 après) |
| `POST /v1/trips/<id>/checkin` | retour confirmé ; après une alerte, les contacts déjà prévenus reçoivent « rentré(e), alerte levée » et les alertes pas encore parties sont annulées |
| `POST /v1/trips/<id>/cancel` ou `DELETE /v1/trips/<id>` | annule avant l'alerte (409 après : confirmer le retour) |

Minuterie : toutes les 15 s, toute sortie active dont l'échéance plus le délai de grâce est passée part en alerte, un message par contact ; échec d'envoi : nouvel essai après 1, 2, 4, 8, 16 puis toutes les 30 minutes, abandon après 24 h (`failed`). Les sorties sont effacées 7 jours après leur fin. Service non configuré (aucun relais SMTP sur le serveur) : 503 `unavailable` pour tous les appels `/v1/trips` ; l'app s'en remet alors à l'alerte par SMS du téléphone.

## Strava (S1)

Détail et étapes pour l'utilisateur : [strava.md](strava.md).

1. `GET /v1/strava/config` → `{"configured", "client_id", "authorize_url", "redirect_uri", "scope"}`.
2. L'app ouvre `authorize_url?client_id=…&redirect_uri=<redirect_uri>&response_type=code&approval_prompt=auto&scope=<scope>&state=<aléa>` (application Strava ou navigateur).
3. Strava renvoie vers `redirect_uri` (`/v1/strava/callback`), qui redirige (302) vers `skimap://strava/callback?code=…&scope=…&state=…` ; l'app vérifie `state`.
4. `POST /v1/strava/token` `{"code"}` → réponse de Strava telle quelle (`access_token`, `refresh_token`, `expires_at`, `athlete`…) ; le serveur y ajoute le secret et ne garde rien.
5. `POST /v1/strava/refresh` `{"refresh_token"}` ; `POST /v1/strava/revoke` `{"token", "token_type_hint"?}` (déconnexion).

60 appels par heure et par adresse ; Strava injoignable : 502 `strava_unreachable` ; non configuré : 503.
