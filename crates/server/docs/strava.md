# Échange OAuth Strava (S1, #62)

[← skimap-server](../README.md)

Strava n'a pas de PKCE et exige `client_secret` pour échanger le code d'autorisation et pour rafraîchir un jeton (L5, [`docs/licences/l5-strava.md`](../../../docs/licences/l5-strava.md)) ; un secret dans l'APK serait extractible et commun à tous (API Policy 5.16 c). Le secret reste donc sur le serveur (secret Kubernetes `skimap-strava`) ; l'app n'a que le `client_id` public, qu'elle lit dans `/v1/strava/config`.

Parcours, appels et erreurs : [api.md](api.md#strava-s1). Le serveur transmet les réponses de Strava telles quelles et ne garde ni ne journalise aucun jeton ni aucune donnée Strava (API Policy 5.5, 5.7, 6.2). La déconnexion utilise `/oauth/revoke` (HTTP Basic avec l'identifiant et le secret), seul point de révocation à partir du 01/06/2027.

## Tests (faux Strava)

`tests/strava.rs` lance un faux `www.strava.com` (`STRAVA_BASE_URL`) : échange du code (corps envoyé à Strava vérifié, secret compris), refus de Strava transmis (400), rafraîchissement, révocation (en-tête Basic vérifié), redirection du rappel vers `skimap://strava/callback` (seuls `code`, `scope`, `state`, `error` passent), entrées invalides jamais transmises, secret absent de toute réponse, Strava injoignable (502), non configuré (503).

## Ce que l'utilisateur doit créer

1. Une application API Strava sur https://www.strava.com/settings/api (compte Strava ; d'après le rapport 06, un abonnement peut être exigé pour créer une app) : nom « skimap », site https://skimap.warnier.net, **Authorization Callback Domain : `skimap.warnier.net`**, icône.
2. Noter le Client ID et le Client Secret, puis créer le secret sur le serveur (commande dans `deploy/README.md`).
3. Une nouvelle app Strava est limitée à 1 athlète (puis 10) ; au-delà, demander la revue de Strava.
4. Le critère du ticket (« connexion Strava depuis l'app de test ») attend l'app (K0, K2) : le schéma `skimap://` doit y être déclaré (filtre d'intention de `skimap://strava/callback`).
