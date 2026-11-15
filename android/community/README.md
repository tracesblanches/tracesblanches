# Module `community` : signalements, notes OSM, « Mes sorties » (V8 à V15)

Signaler en un geste, voir les signalements autour avec leur âge, confirmer ; préparer une note OpenStreetMap que l'utilisateur relit et envoie ; défis et import de l'export Strava. Le module ne connaît pas les liaisons UniFFI : l'app adapte le noyau à `CommunityCore` (`app/.../community/RustCommunityCore.kt`) et le branche dans `MainActivity` (`Community.core`). Serveur : [`crates/server/docs/collab.md`](../../crates/server/docs/collab.md) ; règles : [`crates/collab/docs/regles.md`](../../crates/collab/docs/regles.md).

| Fonction | Comment |
|---|---|
| Signaler (V9) | six gros boutons (72 dp, gants) ; un appui met le signalement en file avec la position et l'heure du geste, sans attendre le réseau ; ensuite, facultatifs : « Ajouter une photo » (aperçu de l'appareil photo, réduit à 1024 px et 60 Ko, sans EXIF) et « Annuler » |
| File hors réseau (V11) | un fichier JSON par signalement (`files/community/outbox/`), un travail WorkManager unique (`APPEND_OR_REPLACE`, réseau requis, reprise exponentielle) ; 410 (déjà expiré) : retiré ; 429, 5xx, pas de réseau : réessayé |
| Autour (V10) | carte du pack (style « fond » instancié par le noyau, hors ligne) avec un point coloré et « Glace · il y a 3 h » par signalement, liste avec confirmations et durée restante, « Toujours là » / « Plus là » (pas sur les siens) |
| Identifiant | 128 bits aléatoires, renouvelés tous les 30 jours ; aucun compte |
| Note OSM (V12) | choix de l'erreur, commentaire facultatif, texte exact affiché, hôte de destination affiché ; envoi anonyme à l'API des notes seulement sur « Envoyer » |
| Mes sorties (V13, V15) | défis (GTJ SRN en entier, toutes les pistes du domaine du pack, km de la saison) calculés sur le téléphone ; archive Strava choisie par l'utilisateur, copiée dans le cache, lue par le noyau, jamais envoyée ; les sorties importées sont apprises par le groupe « Seul » (C11) |

GTJ embarquée : `src/main/assets/challenges.json` (99,8 km, 789 points, DATAtourisme, Licence Ouverte 2.0), régénérée par `cargo run -p skimap-collab --features phone --example challenge_assets -- data/out/france/srn/srn_routes.geojson android/community/src/main/assets/challenges.json`.

## Vérifier

```sh
cd android
./gradlew :community:testDebugUnitTest :community:verifyRoborazziDebug :community:detekt :community:ktlintCheck :community:lintDebug
./gradlew :app:testDebugUnitTest --tests 'fr.skimap.app.community.*'   # vrai skimap-server, vrai noyau
```

Tests JVM : file hors réseau puis envoi avec l'heure du constat (faux serveur et vrai serveur), 410 et 503, travail WorkManager (réessai puis succès), identifiant tournant, photo réduite sans EXIF, note OSM vers un faux serveur des notes (jamais l'API de production), écrans (captures FR, EN sombre), geste → file avec l'heure de l'appui.

## Limites et suites

- Ce qui devrait être automatique et ne l'est pas encore (fichiers d'autres modules) : signalements affichés sur la carte principale près de l'itinéraire du jour (`ui/map/PackMap`), confirmation proposée au passage sur un lieu signalé et défis annoncés en fin de sortie (service `tracking`, bilan `recap`), signalement à la voix (commandes vocales). Le module expose ce qu'il faut : `CommunityApi.list`, `vote`, `ReportOutbox.add`, `CommunityCore.challenges`.
- Le choix « Seul » pour apprendre les sorties Strava est fixe.
