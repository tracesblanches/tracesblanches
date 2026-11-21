# Voix : commandes vocales hors ligne

[Index de la voix](../voix.md)

## Commandes vocales hors ligne (V3 #66, VO5 #87, A11 #98, 15/11/2026)

**Retirées de l'app pour l'instant** (décision du 15/11/2026 au soir : « pas de reconnaissance vocale pour l'instant, c'est compliqué à faire bien ») : le module `android/voicecmd` et `crates/intent` restent dans le dépôt et se testent seuls, mais l'app n'en dépend plus. Ni micro dans le bandeau, ni téléchargement du modèle (un téléchargement mis en file par une version précédente est annulé), ni permission micro ; le bouton du casque garde ses actions (appui : direction et temps restant ; maintenu : « Ramène-moi »). Le branchement dans l'app (micro du bandeau, réponses, téléchargement du modèle) est dans l'historique : commit 95537a2. APK de debug (arm64-v8a et x86_64) : 150 607 563 o avec le module, 92 362 038 o sans (−58,2 Mo : plus de `libsherpa-onnx-jni.so` ni de `libonnxruntime.so`).

Pile STT dans `android/voicecmd` (détails, licences, limites : [android/voicecmd/README.md](../../android/voicecmd/README.md)) : sherpa-onnx 1.13.8, Silero VAD v6.2.3, Parakeet-TDT-0.6B-v3 int8, chargés au geste et libérés après chaque question. Modèle hors APK, téléchargé tout seul avec les packs (Wi-Fi, en charge) : 674 064 268 o (643 Mio). Classification sans LLM dans `crates/intent` (le LLM reste une option non intégrée) ; réponses tirées de la banque (`crates/ffi/src/voicecmd.rs`).

Classification du texte : `test.tsv` (160 questions FR/EN, tenu à l'écart) 99,4 %, `dev.tsv` (161, réglage) 100 % (`cargo test -p skimap-intent --test rate -- --nocapture`, matrice de confusion).

Bout en bout (questions de `test.tsv` dites par une voix de synthèse, deux voix par langue, vent synthétique ; VAD, Parakeet biaisé vers les questions, classifieur), taux de bonne commande :

| | propre | 10 dB | 5 dB | 0 dB | −5 dB |
|---|---|---|---|---|---|
| Toutes (320 par colonne) | 80,0 % | 72,8 % | 70,6 % | 66,3 % | 54,1 % |
| Anglais (`alba`, `peter_yearsley`) | 97,2 % | 95,1 % | 97,9 % | 96,5 % | 88,7 % |
| Français (`estelle`, `marius`) | 66,3 % | 55,1 % | 48,9 % | 42,1 % | 26,4 % |
| dont `estelle` | 83 % | 75 % | 64 % | 53 % | 31 % |
| dont `marius` | 49 % | 35 % | 34 % | 31 % | 21 % |
| Questions hors liste refusées | 97,7 % | 95,5 % | 97,7 % | 97,7 % | 93,2 % |

Les erreurs françaises sont de la transcription, pas de la classification : les questions courtes sortent dans une autre langue (« Tämpsäistä » pour « Temps restant », « Him and more » pour « Ramène-moi ») ; la voix `marius` est mal reconnue même sans vent. Le biais par hotwords n'apporte que 3 points (61,2 → 64,6 % en français propre). Sur le Pixel, ni mesuré ni testé (consigne) : chargement 1,3 à 4,3 s sur le poste.

## Français corrigé (VO5 #87, 21/11/2026)

Parakeet v3 devine la langue au son : les questions françaises courtes sortaient dans une autre langue. Sur le téléphone de langue française, « Bonjour. » (voix de synthèse `estelle`, `android/voicecmd/src/main/resources/fr/skimap/voicecmd/greeting-fr.wav`, 0,44 s) est dit avant la question, puis 0,5 s de silence ; les mots datés avant le milieu de ce silence sont retirés (`GreetingContext`). La question est mise à mi-échelle (`Level`), et les hotwords sont les questions françaises seules. Anglais inchangé (« Hello. » devant perdait 7 points).

Chemin du téléphone sur le poste (`VoiceCommandAudioTest`, `-Dskimap.voicecmd.audio=1`, puis `cargo run --release -p skimap-intent --example rate_heard`), questions françaises du jeu tenu à l’écart (154 commandes, `estelle` et `marius`), bonne commande avant → après :

| | propre | 10 dB | 5 dB | 0 dB | −5 dB |
|---|---|---|---|---|---|
| Commandes françaises (154) | 58,4 → 72,1 % | 50,6 → 69,5 % | 43,5 → 61,7 % | 32,5 → 51,3 % | 14,9 → 32,5 % |
| `marius` (autre locuteur que le « Bonjour », 89) | 43,8 → 60,7 % | 34,8 → 56,2 % | 33,7 → 50,6 % | 28,1 → 48,3 % | 19,1 → 37,1 % |
| Hors liste refusées (FR, 24) | 91,7 → 95,8 % | 83,3 → 95,8 % | 87,5 → 91,7 % | 95,8 → 95,8 % | 100 → 100 % |
| Commandes anglaises (122), inchangées | 97,5 % | 97,5 % | 97,5 % | 94,3 % | 83,6 % |

Le moteur reste chargé d’une question à l’autre dans ce test, mais l’état de Silero VAD est remis à zéro avant chaque question (`VoiceActivity.reset`, `VadResetTest`), comme sur le téléphone où les moteurs sont libérés après chaque question : les taux ne dépendent plus de l’ordre des fichiers (avant ce correctif, jusqu’à 9 points d’écart en anglais à 0 dB). Voix de synthèse, vent synthétique ; vraie voix et vrai vent : à faire (Pierre).
