# Module `tracking` : la sortie sur le téléphone (K4, K5, K6, K8, K13, E1, X10, X11, X14, VO6)

Un seul service de premier plan `location` par sortie, démarré par « Valider la journée » (un geste : il garde la position et le son écran éteint, rapport 09), jamais relancé depuis l'arrière-plan. Le noyau Rust (`crates/ffi/src/tracking.rs`) décide de tout ; le service applique et dit.

Le module ne connaît pas les liaisons UniFFI : l'app adapte le noyau à l'interface `TrackingCore` (`app/.../guidance/RustTrackingCore.kt`) et sous-classe `TrackingService` (`SkiTrackingService`, déclaré dans le manifeste de l'app). Les tests JVM du module tournent avec un noyau factice.

## Ce que fait le service

| Fonction | Comment |
|---|---|
| Positions par lots (E1) | `LocationManagerCompat`, fournisseur `fused` (sinon `gps` ; extra `provider=gps` pour l'A/B du rapport 09), qualité haute, `minUpdateInterval` = intervalle, `setMaxUpdateDelayMillis` = délai du noyau (si ≥ 2 intervalles). Requête rappliquée seulement quand l'intervalle, le délai ou le GNSS changent. Un lot = un appel au noyau, 24 octets par position (`FixPacker`). |
| Modes GPS | ceux du noyau (`gps::decide`) : poche 5 s en lots jusqu'à 60 s, approche et écart 1 s, interaction (écran allumé) 1 s, pause et refuge : requête passive (une position par minute au plus) et capteur de mouvement significatif armé ; réveil 1 s. |
| Écran | `ACTION_SCREEN_ON/OFF` au noyau (mode interaction) ; écran allumé : `requestFlush` (le lot en attente arrive tout de suite). |
| Sobriété | un seul fil (`HandlerThread`) qui dort entre deux lots ; aucune wakelock, aucune minuterie (sauf l'attente bornée des derniers mots après le bilan, 30 s), aucune scrutation. Notification republiée seulement quand son texte change. |
| Annonces (K5) | clips Ogg/Opus du pack décodés (`MediaCodec`), assemblés avec leurs pauses (150 / 400 ms), joués en une fois (`AudioTrack`, usage guidage) avec un focus transitoire « ducking » rendu aussitôt ; voix Android pour toute annonce dont un clip manque (moteur démarré au premier besoin ; `<queries>` TTS_SERVICE). Une annonce à la fois, dans l'ordre. |
| Pas comptés (X10) | compteur de pas d'Android (`TYPE_STEP_COUNTER`, capteur sans réveil, lots de sa FIFO jusqu'à 60 s : il ne réveille jamais le téléphone) ; à chaque lot de positions, `SensorManager.flush`, puis un seul appel au noyau avec le lot et les relevés de 10 s (`StepReports`, silences compris : 0 pas) : `push_batch`. Sans compteur (ou sans réponse au vidage), les positions seules, comme avant. |
| Couches du jour (X11) | `open_tracking` charge les couches du pack (zones sans réseau, couloirs d'avalanche, zones protégées) et les donne au suivi : un itinéraire recalculé les annonce (`TrackingSession.layers()` le dit). |
| Sons courts (K6) | gauche (deux bips aigus à gauche), droite (à droite), hors piste (trois impulsions graves descendantes, au centre), rassurance au retour sur l'itinéraire (un carillon doux) : synthétisés (`EarconSynth`, 140 à 430 ms, crête à −14 dBFS à plein volume, −20 dBFS au volume par défaut de 0,5), stéréo au casque, joués juste avant les mots dans la file de la voix avec le même focus transitoire (« ducking », rendu aussitôt). Réglables : `Tracking.setEarcons(context, enabled, volume)`, lus au départ. |
| Changer de profil (X14) | proposition du noyau dans la notification (« Choisir ce profil » / « Garder le profil », un geste) ; le profil choisi est écrit à côté du plan (`profile_chosen.json`, `onProfileChosen`). |
| Langue (VO6) | textes de la notification et du bandeau en anglais par défaut (`res/values`), en français sur un téléphone en français (`res/values-fr`, `GuidanceLabels.of`). |
| Vibrations (K6) | sécurité : 3 longues ; retour sur l'itinéraire : 1 courte ; gauche : 2 courtes ; droite : 1 longue ; demi-tour : 3 courtes ; proposition : 2 moyennes (usage alarme : senties en silencieux). |
| Boutons (K13) | session média : bouton du casque (appui : prochaine direction et temps restant ; maintien 0,8 s : ramène-moi) ; écran éteint, touches de volume (haut : direction ; bas : temps restant ; maintien : ramène-moi), écran allumé elles règlent le volume. |
| Écran verrouillé | notification en cours promue (Live Update : puce de la barre d'état, progression le long de l'itinéraire) : prochaine direction et distance, temps restant, arrivée et sa fourchette ; actions : accepter / refuser une proposition, garder l'itinéraire après une bascule, ramène-moi. |
| Rappel (K10) | une alarme inexacte (pas de permission d'alarme exacte) avant l'heure de retour ; notification avec le SMS « bien rentré » pré-rempli (l'utilisateur l'envoie). Annulée à la fin de la sortie. |
| Fin | bilan, derniers mots dits, GPX (`trace.gpx`), `observation.json` (le groupe apprend, C11), plan actif effacé. |
| Reprise | le plan actif reste noté ; l'app propose « Reprendre le suivi » (un geste) ; le noyau rejoue `inputs.jsonl` et reprend où il en était. |

## Vérifier

```sh
cd android
./gradlew :tracking:testDebugUnitTest :tracking:detekt :tracking:ktlintCheck :tracking:lintDebug
# Sur l'émulateur ou le Pixel, packs et scénarios Q9 copiés (voir android/README.md) :
cargo run --release -p skimap-ffi --example q9_export -- /tmp/q9 && adb push /tmp/q9/. /data/local/tmp/skimap-q9/
./gradlew :app:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=fr.skimap.app.TrackingReplayTest
./gradlew :app:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=fr.skimap.app.DayFlowTest
adb shell am instrument -w -e class fr.skimap.app.TrackingEnergyTest -e minutes 60 fr.skimap.app.test/androidx.test.runner.AndroidJUnitRunner
adb logcat -s skimap   # « Q9 … », « day flow … », « energy: … »
```

## Limites

- Pas de reconnaissance d'activité (Play services) : la pause se décide sur les positions, les pas comptés (X10) et le mouvement significatif. Le compteur de pas en ski de fond n'est pas mesuré (rapport 09) ; vérifié sous Robolectric et sur émulateur (pas de capteur de pas sur l'émulateur).
- Sons courts : réglage exposé par `Tracking.setEarcons`, pas encore d'écran de réglage dans l'app.
- Annonces dans la langue de la voix installée (VO6 : paquet de voix du site dans la langue du téléphone, sinon celui déjà installé ; voix Android dans la même langue, `TrackingCore.speechLang`). Textes écrits venus du noyau (propositions, causes, bilan) : encore en français.
- Les touches de volume et le bouton du casque n'arrivent à la session que si elle est la session média active (la dernière à avoir joué du son) : une app de musique lancée après prend la main.
- L'énergie réelle se mesure sur le Pixel (rails ODPM, Perfetto) ; sur l'émulateur, seulement le CPU.
