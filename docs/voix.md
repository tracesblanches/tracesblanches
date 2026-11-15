# Voix : pile reprise de speak, banque de phrases, clips des packs

Tickets VO1 (#83), VO2 (#84), VO3 (#85). Décisions : pas de modèle de synthèse sur le téléphone par défaut ; le pipeline génère sur le poste (RTX 5060 Ti) des clips Ogg/Opus livrés dans le pack ; l'app les enchaîne ; la voix Android lit tout texte non prévu. Pas de LLM.

## Repris du projet speak (lecture seule, rien n'est copié dans skimap)

| Élément | Fichier dans `~/projects/speak` | Version | Usage skimap |
|---|---|---|---|
| Moteur pockettts.cpp | `app/scripts/build_native.sh` (clone) | `fab79722` (16/10/2026 « ggml: fork rebase »), sous-module ggml `cc5df49a`, MIT | poste (CUDA) ; téléphone en option (VO4) |
| Patchs 0001 à 0008 | `app/native/patches/*.patch`, registre `app/DEPENDENCIES.md` | 0001 threads + outil de banc, 0002 cible JNI, 0003 décodeur Mimi sur son propre device, 0004 cache de pipelines Vulkan, 0005 et 0006 pool de threads ggml, 0007 crochet de décodage, 0008 correctif Mali Vulkan (llama.cpp PR #30106, à retirer après fusion) | tous appliqués, même arbre que l'app ; dans le build CUDA du poste, 0002 (cible non construite), 0004 et 0008 (Vulkan non compilé) sont inactifs |
| Modèle français 6 couches | `app/models/tts/pocket-tts-french-Q8_0.gguf` | 138 499 840 o, sha256 `e7063830…bd` ; pack d'octobre `kyutai/pocket-tts-without-voice-cloning@08ffc58`, converti par `convert.py` + `quantize` Q8_0 (`app/scripts/fetch_models.sh`) | génération des clips |
| 27 voix françaises | `app/models/tts/pocket-tts-french-voices.gguf` | 164 957 312 o, sha256 `16802ff4…c7` | voix `estelle` |
| Build Android | `app/scripts/build_native.sh`, `app/native/speaktts_jni.cpp` | NDK 28.2.13676358, API 35, Vulkan-Headers `vulkan-sdk-1.4.309.0` | VO4 seulement |
| STT | `app/scripts/fetch_models.sh` | Parakeet-TDT-0.6B-v3 int8, sherpa-onnx 1.13.8, Silero VAD v6.2.3 | VO5 ; ici oracle de contrôle (hors dépôt) |

Mesures speak reprises telles quelles (Pixel 8 Pro, ligne B) : TTS entièrement sur Vulkan, RTF 0,39, premier morceau 210 ms. Coût d'un modèle embarqué : ~140 Mo (133 Mo + une voix extraite des 158 Mo).

## Licences des voix

Source : fiche de `kyutai/pocket-tts-without-voice-cloning@08ffc58` et README de `kyutai/tts-voices` (révision `323332d`). Modèle : CC-BY-4.0 avec les usages interdits de Kyutai (pas de clonage sans consentement, pas de contenu trompeur).

- `estelle` (voix par défaut) : `unmute-prod-website/developpeuse-3.wav`, enregistrement de Kyutai, **CC0**.
- **Non commerciales (CC-BY-NC-4.0), à ne pas utiliser** : `cosette` (Expresso) et aussi **`jean`** (EARS, `ears/p010`), que speak ne signalait pas.
- CC-BY-4.0 : `alba` (Alba MacKenna) ; VCTK : `anna`, `azelma`, `charles`, `eponine`, `eve`, `fantine`, `george`, `jane`, `mary`, `michael`, `paul`, `vera`.
- CC0 : `bill_boerst`, `caro_davy`, `peter_yearsley`, `stuart_bell` (voice-zero), `javert`, `marius` (dons de voix).
- Source non documentée par Kyutai : `giovanni`, `lola` (Common Voice d'après le nom de fichier), `juergen`, `rafael` : à éviter.

## Ce qui manque

- Extraire une seule voix du fichier de 27 voix (VO4, modèle embarqué en option).
- Lecture Ogg/Opus et enchaînement des clips sur Android, énergie par annonce (VO4, K5).
- Licences des noms : ENJ (L1, #6), Biodiv'sports et INPN (L6, #11).

## Pocket TTS sur le poste depuis skimap

```sh
crates/voice/scripts/build-pockettts.sh      # clone fab79722, applique les patchs de speak, build CUDA (~2 min)
# -> ~/.cache/skimap/pockettts/build-cuda/pocket-tts (hors dépôt)
SKIMAP_DATA_DIR=~/projects/skimap/data cargo run --release -p skimap-voice --example voice_pack -- \
    ~/projects/skimap/research/raw            # les deux sites ; ou : ... raw cervieres
cargo test -p skimap-voice --test pocket -- --ignored   # contre le vrai modèle
```

Variables (défauts de `VoiceConfig::from_env`) : `SKIMAP_POCKETTTS_BIN`, `SKIMAP_POCKETTTS_MODEL` et `SKIMAP_POCKETTTS_VOICES` (fichiers de speak), `SKIMAP_VOICE=estelle`, `SKIMAP_TTS_DEVICE`, `SKIMAP_TTS_SEED=42`, `SKIMAP_FFMPEG`, `SKIMAP_OPUS_KBPS=24`.

Appel par sous-processus et non par FFI : le workspace interdit `unsafe`, et lier la bibliothèque C++/CUDA attacherait la compilation cargo (et celle du téléphone) à CUDA. Un seul processus par pack : le modèle se charge une fois (0,2 à 0,3 s), puis `--stream-by-line` produit un WAV par ligne ; le flux est découpé exactement grâce au nombre de trames de chaque ligne écrit dans le journal.

## Banque de phrases (VO2)

`skimap_voice::bank::Event::announce` donne une annonce par événement du noyau (direction, bifurcation, arrivée, distance et temps restants, demi-tour, pente, zone protégée et règles, zone sans réseau, écart et retour, rassurance, briefing, équipement, fartage, tronçon avalancheux, changement de conditions, batterie, résumé de boucle suggérée). Une annonce est une suite de clips ; chaque clip est une unité parlée complète finie par un point (le modèle donne une intonation finale à chaque clip), avec une pause après (150 ms dans un message, 400 ms entre deux). Seule exception, le résumé de boucle (A5) : une phrase en trois clips, « Boucle de treize kilomètres, » / « environ deux heures, » / « retour vers midi et quart. » (longueur 1 à 60 km, durées de la grille jusqu'à 12 h, retour au quart d'heure dit comme on le dit : « midi et quart », « seize heures moins le quart »).

- Nombres en toutes lettres (`numbers`), arrondis sur des grilles finies (`quantity`) : distances (20 m à 100 km), durées (1 min à 24 h), heures au quart, pentes 30 à 45°, températures −35 à 30 °C, vent par 10 km/h. Comptes à rebours et batterie arrondis vers le bas.
- Clips : phrases fixes (76 depuis le 15/11/2026, dont « Bulletin d'avalanche non disponible hors ligne. Consultez-le avant de partir. », « Des avalanches restent possibles sur les pentes raides. », « Votre itinéraire traverse des zones d'avalanche connues. », « Votre itinéraire traverse une zone protégée. », « Une partie de l'itinéraire est sans aucun réseau, même pour les appels d'urgence. », « Feu interdit sur la commune, réchauds compris. », « Neige froide et sèche. » et trois éléments d'équipement), valeurs (757 à 836 selon le nombre de pistes, dont 170 pour les résumés de boucle), noms du pack (`piste.<nom>.<couleur>` : « Greffier, piste noire. », `name.<nom>`), dates de fin des périodes de protection. `manifest(&PackNames)` donne l'ensemble fini id → texte.
- Une valeur hors grille ou un nom absent du pack a un texte correct mais un id absent : l'app lit alors l'annonce entière avec la voix Android.
- Morceaux : `Announcement::chunks(50, …)` regroupe les clips (fins de phrase) en morceaux de 50 tokens au plus. Le comptage de tokens est un portage exact de pockettts.cpp (préparation du texte du pack français, Unigram avec repli sur octets) : il donne le même nombre que pockettts.cpp pour chacun des 1 668 clips générés.

## Clips des packs (VO3), mesurés le 14/11/2026

Contrôle qualité : tokens identiques à ceux de pockettts.cpp ; fin de parole atteinte ; si la parole commence avant 60 ms (premier phonème coupé par la trame de démarrage que pockettts.cpp supprime, « Greffier » entendu « véhier »), nouvel essai avec la graine suivante (8 au plus) ; suppression du son coupé que la dernière trame commence après la fin de parole ; silence de début et de fin coupé (40 ms gardés avant, 80 ms après) ; niveau normalisé à −20 dBFS, crête ≤ −1 dBFS ; Opus 24 kbit/s, 24 kHz mono.

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Noms (pistes, lieux, dates) | 21, 31, 4 | 118, 107, 1 |
| Clips (fixes, valeurs, noms) | 703 (66, 585, 52) | 965 (66, 679, 220) |
| Taille des clips (somme des fichiers) | 2,94 Mo | 3,95 Mo |
| Audio | 959 s | 1 291 s |
| Génération : TTS (GPU) + Opus = total | 17,0 + 2,1 = 19,3 s | 22,7 + 2,9 = 25,8 s |
| Clips refaits (graines 43, 44, 45) | 37, 5, 2 | 53, 9, 1 |

Le 15/11/2026, noms tirés des couches construites (`skimap voice <site>` : pistes de la couche `pistes`, refuges de `refuges`, zones naturelles de `zones` ; plus les noms ENJ ni les zones INPN absentes de la couche) et phrases nouvelles : Cervières 23 pistes, 28 lieux, 4 dates, 884 clips (76, 757, 51), 3,68 Mo, 1 195 s d'audio, 21,7 + 2,6 = 24,6 s ; Chapelle-des-Bois 105, 106, 1, 1 123 clips (76, 836, 211), 4,58 Mo, 1 492 s, 26,8 + 3,3 = 30,4 s ; aucun clip à l'attaque coupée. Transcription (Parakeet v3) des dix phrases nouvelles et de quatre résumés de boucle assemblés : WER 6,8 % (10 mots sur 147), erreurs « pentes raides » → « panther aides », « fart » → « phare », « peaux » → « pots », « ciel » → « sel », « midi et quart » → « midi écart ».

Travail complet (deux packs et 20 échantillons) : 50,8 s. Silence de début coupé (pack Cervières) : `estelle` médiane 90 ms, maximum 310 ms ; il dépend de la voix (médianes `alba` 80, `juergen` 240, `rafael` 320, `lola` 370 ms), d'où une coupe par énergie et non fixe. Les 300 à 700 ms de speak venaient de la version Python, voix anglaises. Le contrôle d'attaque est réglé pour `estelle` (0 clip signalé) ; il en laisse 8 avec `alba`, 57 avec `juergen` et 147 avec `rafael`.

Échantillons à écouter (`.opus`, et `.wav` à côté), assemblés comme sur le téléphone (décodage des clips du pack, pauses) : `data/out/<site>/voice/samples/01-…` à `10-briefing`, liste et textes dans `samples.json`. Les noms sont ceux des packs ; distances, heures et météo sont des valeurs d'exemple.

Contrôle par transcription (Parakeet v3, oracle Python hors dépôt, nombres réécrits en lettres) : WER 8,8 % (Cervières) et 5,8 % (Chapelle) sur les échantillons. Les erreurs restantes sont des homophones (« tournez » / « tourner », « fart » / « phare ») et des noms propres (« Gibera », « Sioux », « Thieulette »). Trois formulations ont été changées sur cette mesure : « afin de rentrer » (au lieu de « pour rentrer », entendu « pour entrer »), « Risque d'avalanche deux sur cinq, limité » (« avalanche limité » entendu « avalanche unité » 6 fois sur 6) et « Le vent sera faible ».
