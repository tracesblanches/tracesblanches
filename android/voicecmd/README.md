# Module `voicecmd` : commandes vocales hors ligne (V3 #66, VO5 #87, A11 #98)

Une question à la fois, après un geste (bouton du casque maintenu, voir « Limites » ; sans casque, le micro du bandeau de guidage), sans réseau, sans aucun réglage : « Où suis-je ? », « Temps restant ? », « Prochaine direction ? », « Ramène-moi », « Heure de demi-tour ? », « Signale : trace faite », « Signale : piste damée », « Combien de batterie ? », « Répète », en français et en anglais.

## Chaîne

| Étape | Où | Comment |
|---|---|---|
| Micro | `MicSource` | `AudioRecord` 16 kHz mono, source `VOICE_RECOGNITION`, ouvert au geste, fermé à la fin de la question ; les trames sont gardées pendant le chargement des moteurs (`VoiceCommands.Capture`) |
| Fin de la question | `Endpointer` + `SileroVad` | Silero VAD v6.2.3 (sherpa-onnx `Vad.compute`, 64 échantillons de contexte comme speak), trames de 32 ms : parole après 3 trames ≥ 0,5 ; fin après 700 ms < 0,35 ; coupure à 8 s ; rien en 5 s : abandon ; 300 ms gardées avant la parole |
| Transcription | `ParakeetTranscriber` | Parakeet-TDT-0.6B-v3 int8, sherpa-onnx 1.13.8 `OfflineRecognizer`, recherche en faisceau biaisée vers les questions de la liste (hotwords, score 2 comme speak), 500 ms de silence devant (speak : sinon Parakeet perd ou déforme le début) |
| Classification | le noyau (`crates/intent`, via `crates/ffi/src/voicecmd.rs`) | sans LLM : normalisation, lexique FR/EN de synonymes, distance d'édition, mots-clés ; liste fermée ; sinon « je n'ai pas compris » et la liste des questions |
| Réponse | l'app (`app/.../voicecmd/CommandAnswers`, retiré de l'app le 15/11/2026, dans l'historique : commit 95537a2) | toujours un appel du noyau ou une phrase de la banque vocale, dite par le service de la sortie (clips du pack, voix Android pour ce qui n'a pas de clip) |
| Libération | `VoiceCommands.listen` | les deux moteurs sont libérés après chaque question (batterie) : rien ne reste chargé entre deux commandes |

Le module ne connaît pas les liaisons UniFFI : l'app adapte le classifieur du noyau à `CommandClassifier` (`RustCommandClassifier`) ; les tests JVM du module tournent avec des moteurs factices, et ceux de l'app avec les vrais (sherpa-onnx JNI pour Linux x86_64, même version) et le vrai classifieur (bibliothèque Rust de l'hôte).

## Modèle : paquet facultatif, pas dans l'APK

`ModelSpec.PARAKEET_V3_INT8`, téléchargé tout seul avec les packs, sans rien demander (`VoiceModelWorker`, mis en file par la mise à jour quotidienne des packs : Wi-Fi, en charge, stockage suffisant) par `ModelInstaller` dans `files/voicecmd/parakeet-tdt-0.6b-v3-int8/` : fichier par fichier, reprise par `Range` sur le `.part`, sha256 vérifié avant de prendre son nom, marqueur `SHA256SUMS` à la fin ; supprimable (`ModelStore.delete`).

| Fichier | Source (révision figée) | Octets |
|---|---|---|
| `encoder.int8.onnx` | Hugging Face `csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8@ed5d745` | 652 282 294 |
| `decoder.int8.onnx` | idem | 11 845 274 |
| `joiner.int8.onnx` | idem | 6 355 277 |
| `tokens.txt` | idem (identique à celui de speak) | 93 939 |
| `tokenizer.json` | `nvidia/parakeet-tdt-0.6b-v3@541d1f9` (comme speak) ; `bpe.vocab` en est tiré sur le téléphone (`BpeVocab`, identique octet pour octet à celui de speak) | 1 159 960 |
| `silero_vad.onnx` | `snakers4/silero-vad` v6.2.3 | 2 327 524 |
| **Total** | | **674 064 268 (643 Mio)** |

Les fichiers ONNX du miroir Hugging Face ne sont pas ceux de l'archive `asr-models` de sherpa-onnx que speak a validée (tailles différentes, même tokens.txt) : l'archive est en `.tar.bz2`, que le téléphone ne sait pas ouvrir sans bibliothèque de plus ; le test de bout en bout ci-dessous valide les fichiers que le téléphone télécharge.

**Dans l'APK** : seulement `libsherpa-onnx-jni.so` et `libonnxruntime.so` de l'AAR sherpa-onnx 1.13.8 (sha256 publié par GitHub, vérifié par la tâche `sherpaAar`), pour arm64-v8a et x86_64 ; les bibliothèques `c-api` et `cxx-api` de l'AAR ne sont pas embarquées (`readelf` : le JNI n'en dépend pas). Mesure de l'APK : voir « Mesures ».

## Licences (vérifiées le 15/11/2026)

| Composant | Licence | Compatible GPL-3.0-or-later avec l'exception des boutiques |
|---|---|---|
| sherpa-onnx 1.13.8 (bibliothèque, JNI, API Kotlin) | Apache-2.0 | oui (Apache-2.0 → GPLv3) |
| onnxruntime (dans l'AAR) | MIT | oui |
| Silero VAD v6.2.3 (modèle) | MIT | oui |
| Parakeet-TDT-0.6B-v3 (NVIDIA, modèle ; export sherpa-onnx) | CC-BY-4.0 | oui : c'est une donnée téléchargée à part, pas du code lié ; attribution due (écran « Licences » de l'app) |

L'exception des boutiques ne porte que sur le code de skimap ; aucune de ces licences ne l'interdit ni ne l'exige.

## Vérifier

```sh
cd android
./gradlew :voicecmd:testDebugUnitTest :voicecmd:lintDebug :voicecmd:detekt :voicecmd:ktlintCheck
# Bout en bout sur le poste (modèle et audio dans ~/.cache/skimap/voicecmd) :
cargo run --release -p skimap-intent --example command_audio -- ~/.cache/skimap/voicecmd/audio
./gradlew :app:testDebugUnitTest --tests 'fr.skimap.app.voicecmd.*'      # SKIMAP_VOICECMD_SNR=clean : une seule colonne
```

Le premier test de l'app installe le modèle depuis `~/.cache/skimap/voicecmd/` avec `ModelInstaller` (sha256 des fichiers en place, comme le téléphone) ; sans modèle ni audio, il est sauté.

## Mesures

Voir le commentaire des tickets #87 et #98 et `docs/voix/commandes-vocales.md` (le module n'est plus dans l'app depuis le 15/11/2026).

## Limites

- Bouton du casque maintenu : il appartient au module `tracking` (`ButtonGestures`, aujourd'hui « ramène-moi ») ; le brancher sur `VoiceCommandController.ask` est une demande à l'orchestrateur. Le micro du bandeau marche dès que le modèle est installé.
- Banque vocale en français seulement : une question en anglais est comprise, et les réponses propres à la commande (pas compris, signalement…) sont en anglais, mais les réponses du guidage (temps restant, direction…) restent celles de la banque française, comme le reste du guidage.
- Signalements : pas de module communauté dans l'app ; la commande est reconnue et l'appel préparé (`Reports`), la réponse dit « pas encore disponibles ».
- Autonomie de la batterie : aucun estimateur sur le téléphone ; « Combien de batterie ? » dit le niveau seul.
- Interface ajoutée : le micro du bandeau (seulement quand le modèle est là), pour un téléphone sans casque ; et la demande système du micro au premier usage (Android ne l'accorde pas sans l'utilisateur). Rien d'autre : ni réglage ni dialogue de téléchargement.
- Mesures sur le poste (CPU x86_64) et voix de synthèse ; le Pixel n'a pas été touché.
