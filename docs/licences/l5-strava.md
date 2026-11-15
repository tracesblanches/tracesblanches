# L5 : Strava (OAuth, import d'historique, clauses de l'API)

Ticket #10. Retour au [sommaire](../licences-donnees.md). Lu le 15/11/2026.

## OAuth : pas de PKCE documenté, secret client obligatoire

Source : https://developers.strava.com/docs/authentication/ et https://developers.strava.com/docs/changelog/.
- Aucune occurrence de « PKCE », « code_challenge » ou « code_verifier » sur ces deux pages.
- « A registered application will be assigned a client ID and client secret. The secret is used for authentication and should never be shared. »
- OAuth mobile : `GET https://www.strava.com/oauth/mobile/authorize` (ouvre l'app Strava, version 75.0 ou plus, sinon le web) ; « Mobile OAuth supports refresh tokens and short-lived access tokens. Mobile OAuth does not support forever access tokens. »
- Échange du code : « The application must now call the POST https://www.strava.com/oauth/token with its client ID and client secret » ; paramètre `client_secret` « required ». Rafraîchissement (jeton d'accès valable six heures) : `client_secret` « required » aussi. Révocation (`/oauth/revoke`, depuis le 01/06/2026) : authentification HTTP Basic avec `client_id` et `client_secret`.
- API Policy 5.16 (c) : interdit de « share, transfer, multiplex, or otherwise re-use API Tokens or authentication credentials across multiple servers, services, applications, or end users ». Un secret embarqué dans un APK public est extractible et commun à tous les utilisateurs : à éviter (lecture prudente de la clause).

Conséquences :
- **Usage personnel** : l'utilisateur crée sa propre application API Strava (abonnement Strava requis pour créer une app, d'après la page des quotas citée au rapport 06) et saisit son `client_id` et son `client_secret`, stockés sur son téléphone.
- **App publique** : un petit service d'échange de jetons qui garde le secret (échange du code et rafraîchissement), ou pas d'OAuth Strava du tout. Une nouvelle app est limitée à 1 athlète, puis 10, au-delà il faut une revue (rapport 06).

## Clauses en vigueur (API Agreement et API Policy, « Effective Date: June 1, 2026 »)

Textes : https://www.strava.com/legal/api et https://www.strava.com/legal/api_policy.
- Agreement 7.1 : « Your Developer Applications may include the option to upload activities or information to the Strava Platform » : l'envoi d'une sortie est permis.
- Policy (affichage) : « Strava Data provided by a specific Strava user may be displayed or disclosed in your Developer Application only to that user. »
- Policy 5.2 : pas d'usage « competitive to Strava or the Strava Platform ».
- Policy 5.3 : pas d'usage « in connection with the development, training, evaluation, or operation of any AI Application », y compris pour les données dérivées, agrégées ou anonymisées.
- Policy 5.4 : pas de traitement « for the purposes of analytics, analyses », même agrégé ou désidentifié.
- Policy 5.5 : pas d'export massif ni de stockage de Strava Data « or any data derived from Strava Data, in any Persistent Index » ; seul le cache de 7 jours (6.2) est admis.
- Policy 5.7 : pas de stockage d'informations géographiques ou d'utilisateur, sauf 6.2.
- Policy 5.8 : « You may not charge end users, in any manner, for access to or use of the Strava API Materials or any services or functionality included in or related to the Strava API Materials » ; permis : faire payer « functionality not provided by the Strava Platform […] and that is not substantially duplicative of functionality offered by Strava ». Une app vendue une fois devrait donc garder l'envoi vers Strava hors de ce qui est payé.
- Policy 6.2 : « You may not retain Strava Data in your cache for longer than seven (7) days ».
- Policy 6.6 : « Each Strava user has the right to access and export the user's own Strava data, free of charge, through the Bulk Data Export Tool […]. Nothing in this Agreement is intended to limit or condition that user-facing right. »

## Interprétation retenue (inchangée depuis le rapport 06)

- Historique pour calibrer le temps estimé : **import par l'utilisateur de son export Strava** (fichiers GPX/FIT de l'archive), hors API, donc hors des clauses 5.3, 5.4, 5.5 et 6.2 ; c'est une lecture du texte, **non validée juridiquement**.
- API : seulement l'envoi d'une sortie (`activity:write`), sans lecture ni conservation de Strava Data.
