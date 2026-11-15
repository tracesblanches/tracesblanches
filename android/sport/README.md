# Module `sport` : envoyer la sortie vers Health Connect et Strava (K11)

À la fin d'une sortie, le bilan (écran du plan) propose « Envoyer » ; le menu de la carte ouvre « Envoyer mes sorties » (Health Connect, Strava, « Envoyer automatiquement à la fin de la sortie », sorties envoyées et où elles en sont). Rien n'est envoyé sans geste de l'utilisateur ou sans l'option automatique.

| Fonction | Comment |
|---|---|
| Sortie | `Outing.of(planDir, gpx, bilan, km, sport)` : le GPX écrit par le noyau à la fin (`trace.gpx`), nom de la boucle, description = bilan + « Conditions : » (briefing A1 et fartage A2 du matin, lus dans `plan.json`) + signature. Type Strava : `NordicSki`, ou `BackcountrySki` quand la pratique choisie est la randonnée nordique (SRN). |
| Health Connect | `ExerciseSessionRecord` type `SKIING` (Health Connect n'a qu'un type de ski) avec son `ExerciseRoute` (tous les points, fin = dernier point + 1 s), `clientRecordId` stable : renvoyer la même sortie la remplace. Permissions d'écriture seulement (`WRITE_EXERCISE`, `WRITE_EXERCISE_ROUTE`) ; rien n'est jamais lu. Écran de consentement : la demande de Health Connect, et la page de justification (`ACTION_SHOW_PERMISSIONS_RATIONALE`, alias `VIEW_PERMISSION_USAGE` sur Android 14+). |
| Strava : connexion | Strava n'a pas de PKCE et exige le secret : l'échange passe par skimap-server (`/v1/strava/config`, `token`, `refresh`, `revoke`, voir `crates/server/docs/strava.md`). Page d'autorisation dans le navigateur avec un `state` aléatoire, retour par `skimap://strava/callback` (activité `singleTask`), `state` et portée `activity:write` vérifiés. |
| Strava : envoi | `POST /uploads` (GPX, nom, description, `external_id`), suivi de `GET /uploads/{id}` jusqu'à l'activité, puis `PUT /activities/{id}` avec `sport_type` et la description. Doublon reconnu (l'activité existante est reprise). Jeton rafraîchi 2 min avant expiration. |
| File hors ligne | `files/sport/queue/<id>.json` + copie du GPX ; WorkManager : Health Connect tout de suite, Strava avec `NetworkType.CONNECTED` (attend le réseau, survit au redémarrage), reprise exponentielle à partir de 30 s si Strava est injoignable, traite encore, ou limite (429, 5xx). Un fichier refusé est un échec, pas une reprise. |
| Déconnexion | « Déconnecter Strava » : révocation par le serveur, puis jeton retiré du téléphone (même sans réseau). Autorisation retirée sur strava.com (401) : jeton effacé, la sortie attend une nouvelle connexion ; après connexion ou autorisation Health Connect, les sorties en attente partent (`Sport.resume`). |

Aujourd'hui, https://skimap.warnier.net répond `configured: false` (pas encore d'application Strava) : l'écran dit « La connexion à Strava sera possible quand le service skimap sera configuré pour Strava. » et n'affiche pas de bouton.

## Vérifier

```sh
cd android
./gradlew :sport:testDebugUnitTest :sport:verifyRoborazziDebug :sport:lintDebug :sport:detekt :sport:ktlintCheck
# Émulateur Android 14+ (Health Connect intégré) :
./gradlew :sport:connectedDebugAndroidTest
adb logcat -s skimap   # « K11 health connect: … », « K11 strava: … »
```

Les tests JVM et instrumentés partagent `src/sharedTest` : `FakeStrava` (faux skimap-server et faux API Strava sur un port local, jeton porteur vérifié, requêtes enregistrées) et `SportFixtures` (GPX au format du noyau, `plan.json` avec son matin). Captures : `src/test/screenshots/sport_*.png`.

## Limites

- Sans application Strava réelle, l'envoi n'est prouvé que contre le faux Strava ; le critère « sortie réelle visible sur Strava » attend les identifiants (S1).
- Le GPX du noyau n'a pas d'altitude : ni Strava ni Health Connect n'en reçoivent (Strava la recalcule).
- FIT non produit : le GPX suffit à Strava.
