# Voix : pile reprise de speak, licences, Pocket TTS sur le poste

[Index de la voix](../voix.md)

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
- Licences des noms : ENJ (L1, #6), Biodiv'sports et INPN (L6, #11). En attendant, les clips des noms lus seulement dans une source à usage personnel sont marqués et restent hors du pack publié (ci-dessous, « Voix publique et voix personnelle »).

## Pocket TTS sur le poste depuis skimap

```sh
crates/voice/scripts/build-pockettts.sh      # clone fab79722, applique les patchs de speak, build CUDA (~2 min)
# -> ~/.cache/skimap/pockettts/build-cuda/pocket-tts (hors dépôt)
SKIMAP_DATA_DIR=~/projects/skimap/data cargo run --release -p skimap-voice --example voice_pack -- \
    [--names] [cervieres|chapelle-des-bois]   # noms des couches construites ; --names : noms seuls
cargo test -p skimap-voice --test pocket -- --ignored   # contre le vrai modèle
```

Variables (défauts de `VoiceConfig::from_env`) : `SKIMAP_POCKETTTS_BIN`, `SKIMAP_POCKETTTS_MODEL` et `SKIMAP_POCKETTTS_VOICES` (fichiers de speak), `SKIMAP_VOICE=estelle`, `SKIMAP_TTS_DEVICE`, `SKIMAP_TTS_SEED=42`, `SKIMAP_FFMPEG`, `SKIMAP_OPUS_KBPS=24`.

Appel par sous-processus et non par FFI : le workspace interdit `unsafe`, et lier la bibliothèque C++/CUDA attacherait la compilation cargo (et celle du téléphone) à CUDA. Un seul processus par pack : le modèle se charge une fois (0,2 à 0,3 s), puis `--stream-by-line` produit un WAV par ligne ; le flux est découpé exactement grâce au nombre de trames de chaque ligne écrit dans le journal.
