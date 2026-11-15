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

## Banque de phrases (VO2)

`skimap_voice::bank::Event::announce` donne une annonce par événement du noyau (direction, bifurcation, arrivée, distance et temps restants, demi-tour, pente, zone protégée et règles, zone sans réseau, écart et retour, rassurance, briefing, équipement, fartage, tronçon avalancheux, changement de conditions, batterie, résumé de boucle suggérée). Une annonce est une suite de clips ; chaque clip est une unité parlée complète finie par un point (le modèle donne une intonation finale à chaque clip), avec une pause après (150 ms dans un message, 400 ms entre deux). Exceptions : les propositions de C11 (« Je vous propose la boucle courte, » puis « retour vers quinze heures quinze. », `Fixed::continued`) et le résumé de boucle (A5) : une phrase en trois clips, « Boucle de treize kilomètres, » / « environ deux heures, » / « retour vers midi quinze. » (longueur 1 à 60 km, durées de la grille jusqu'à 12 h, retour au quart d'heure dit comme on le dit : « midi quinze », « treize heures et demie », « seize heures moins le quart » ; « et quart » remplacé par « quinze » le 15/11/2026, ci-dessous).

- Nombres en toutes lettres (`numbers`), arrondis sur des grilles finies (`quantity`) : distances (20 m à 100 km), durées (1 min à 24 h), heures au quart, pentes 30 à 45°, températures −35 à 30 °C, vent par 10 km/h. Comptes à rebours et batterie arrondis vers le bas.
- Clips : phrases fixes (91 depuis le 15/11/2026, dont les quinze du plan de journée `fix.plan-*` : huit de J1 à J3, sept des propositions de C11, et « Bulletin d'avalanche non disponible hors ligne. Consultez-le avant de partir. », « Des avalanches restent possibles sur les fortes pentes. », « Votre itinéraire traverse des zones d'avalanche connues. », « Votre itinéraire traverse une zone protégée. », « Une partie de l'itinéraire est sans aucun réseau, même pour les appels d'urgence. », « Feu interdit sur la commune, réchauds compris. », « Neige froide et sèche. » et trois éléments d'équipement), valeurs (757 à 836 selon le nombre de pistes, dont 170 pour les résumés de boucle), noms du pack (`piste.<nom>.<couleur>` : « Greffier, piste noire. », `name.<nom>`), dates de fin des périodes de protection. `manifest(&PackNames)` donne l'ensemble fini id → texte.
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

Le 15/11/2026, noms tirés des couches construites (`skimap voice <site>` : pistes de la couche `pistes`, refuges de `refuges`, zones naturelles de `zones` ; plus les noms ENJ ni les zones INPN absentes de la couche) et phrases nouvelles : Cervières 23 pistes, 28 lieux, 4 dates, 884 clips (76, 757, 51), 3,68 Mo, 1 195 s d'audio, 21,7 + 2,6 = 24,6 s ; Chapelle-des-Bois 105, 106, 1, 1 123 clips (76, 836, 211), 4,58 Mo, 1 492 s, 26,8 + 3,3 = 30,4 s ; aucun clip à l'attaque coupée. Transcription (Parakeet v3) des dix phrases nouvelles et de quatre résumés de boucle assemblés : WER 6,8 % (10 mots sur 147), erreurs « pentes raides » → « panther aides », « fart » → « phare », « peaux » → « pots », « ciel » → « sel », « midi et quart » → « midi écart ». Le même jour, « sur les pentes raides » est devenu « sur les fortes pentes » (même sens). La phrase d'origine, synthétisée seule avec les graines 42 à 61 et transcrite, n'est juste que pour 2 graines sur 20 (51 et 53 ; ailleurs « pentrèdes », « pentherades », « pent-red »…). Choisir la graine par transcription mettrait Python (sherpa-onnx, seule STT du poste) dans le générateur, et resterait fragile. Sur les mêmes 20 graines : « sur les fortes pentes » 20/20, « sur les pentes les plus raides » 18/20, « sur les pentes fortes » 13/20. L'audio d'une ligne ne dépend que de sa graine, pas des lignes qui la précèdent dans le processus (empreintes identiques). Packs refaits (24,9 s à Cervières, 30,4 s à Chapelle-des-Bois, mêmes nombres de clips, aucune attaque coupée) ; transcription de tous les clips, chacun seul : phrases fixes 11,5 % (49 mots sur 425) sur chaque site, la phrase changée exacte ; valeurs 4,4 % (163/3 747) et 4,6 % (186/4 083) ; noms 41,1 % (78/190) et 28,9 % (243/841, surtout des noms propres) ; phrases nouvelles et résumés de boucle assemblés, 5,4 % (8/147) sur les deux sites, au lieu de 6,8 %.

Phrases du plan de journée (`fix.plan-*`, 15/11/2026) : packs refaits, 892 clips à Cervières (84, 757, 51) et 1 131 à Chapelle-des-Bois (84, 836, 211), les 884 et 1 123 clips existants inchangés (manifeste identique : durée, taille, graine), aucune attaque coupée ; 26,8 s et 34,2 s (TTS 23,7 et 29,3 s). Chaque phrase synthétisée seule avec les graines 42 à 61 et transcrite (homophones confondus : « nouvelle » / « nouvel », « sorties » / « sortie ») : « Boucle écourtée » (6/20, « courtée », « l'horreur prévue ») est devenue « Vous êtes en retard sur l'heure prévue. Boucle raccourcie pour rentrer à l'heure. » (18/20) ; « Itinéraire recalculé. » (15/20, « Itinéraire calculé ») « Nouvel itinéraire calculé. » (20/20) ; « Rentrez maintenant… » (15/20, « Entrez ») « Il faut rentrer maintenant par le plus court, afin d'arriver avant la nuit. » (20/20) ; les cinq autres de 17/20 à 20/20. Clips des packs, chacun seul : WER 3,6 % (2/56, « raccourci », « Nouvelle »), 1,2 % après « Toujours sur l'itinéraire. » ; plans J1 des deux sites : 0 clip absent.

Travail complet (deux packs et 20 échantillons) : 50,8 s. Silence de début coupé (pack Cervières) : `estelle` médiane 90 ms, maximum 310 ms ; il dépend de la voix (médianes `alba` 80, `juergen` 240, `rafael` 320, `lola` 370 ms), d'où une coupe par énergie et non fixe. Les 300 à 700 ms de speak venaient de la version Python, voix anglaises. Le contrôle d'attaque est réglé pour `estelle` (0 clip signalé) ; il en laisse 8 avec `alba`, 57 avec `juergen` et 147 avec `rafael`.

Échantillons à écouter (`.opus`, et `.wav` à côté), assemblés comme sur le téléphone (décodage des clips du pack, pauses) : `data/out/<site>/voice/samples/01-…` à `10-briefing`, liste et textes dans `samples.json`. Les noms sont ceux des packs ; distances, heures et météo sont des valeurs d'exemple.

Contrôle par transcription (Parakeet v3, oracle Python hors dépôt, nombres réécrits en lettres) : WER 8,8 % (Cervières) et 5,8 % (Chapelle) sur les échantillons. Les erreurs restantes sont des homophones (« tournez » / « tourner », « fart » / « phare ») et des noms propres (« Gibera », « Sioux », « Thieulette »). Trois formulations ont été changées sur cette mesure : « afin de rentrer » (au lieu de « pour rentrer », entendu « pour entrer »), « Risque d'avalanche deux sur cinq, limité » (« avalanche limité » entendu « avalanche unité » 6 fois sur 6) et « Le vent sera faible ».

## Propositions de C11, demi-tour avancé, voix publique (15/11/2026)

**Phrases nouvelles** (`Fixed::PlanPace*`, `Fixed::PlanPropose*`, `Fixed::PlanProposalAccepted`, textes exacts demandés par C11) : « Vous allez moins vite que prévu. », « Vous allez moins vite que prévu, avec plus de pauses. », « Vous faites plus de pauses que prévu. », « Je propose la boucle courte, », « Je propose la sortie la plus proche, », « Je propose de garder la boucle, », « Proposition acceptée. » ; 8 à 16 jetons. `skimap-plan` les prend dans la banque (plus de clip en attente). **Phrase corrigée** : `fix.turnaround-late`, « Vous êtes en retard sur l'horaire prévu. L'heure de demi-tour est avancée. » (entendue « l'horreur prévue … Donitur ») devient « Vous êtes en retard sur l'heure prévue. Le demi-tour est avancé. ».

Transcription (Parakeet v3), chaque phrase seule, graines 42 à 61 (« homophones confondus » : pause / pauses, prévu / prévue) :

| Phrase | Exactes /20 (homophones confondus) | WER (homophones confondus) |
|---|---|---|
| Vous êtes en retard sur l'horaire prévu. L'heure de demi-tour est avancée. (ancienne) | 7 (13) | 7,3 % (3,5 %) |
| Vous êtes en retard sur l'heure prévue. L'heure de demi-tour est avancée. | 11 (13) | 3,5 % (2,7 %), « L'heure demi-tour » |
| **Vous êtes en retard sur l'heure prévue. Le demi-tour est avancé.** | **17 (19)** | **1,7 % (0,4 %)** |
| Vous êtes en retard sur l'heure prévue. Il faudra faire demi-tour plus tôt. | 14 (15) | 2,9 % (1,8 %) |
| Vous êtes en retard sur l'heure prévue. Faites demi-tour plus tôt que prévu. | 17 (18) | 1,4 % (0,7 %) |
| Vous allez moins vite que prévu. | 20 (20) | 0 % |
| Vous allez moins vite que prévu, avec plus de pauses. | 0 (20) | 10 % (0 %), « pause » |
| Vous faites plus de pauses que prévu. | 0 (20) | 14,3 % (0 %), « pause » |
| Je propose la boucle courte, | 18 | 2,0 % |
| Je propose la sortie la plus proche, | 16 | 2,9 %, « Je vous propose » |
| Je propose de garder la boucle, | 15 | 4,2 %, « Je vous propose » |
| Proposition acceptée. | 20 | 0 % |

Les huit phrases : WER 5,0 % (55 mots sur 1 100), 1,1 % (12) homophones confondus ; les sept nouvelles 5,9 % (1,3 %).

**Enchaînement** « raison » + « Je propose …, » + `loop.back.<HHMM>` existant, comme le téléphone l'assemble (clips seuls, silences coupés, 400 ms puis 150 ms), graines 42 à 61, heures de la STT réécrites (« 15h15 » = « quinze heures et quart ») :

| Annonce | Exactes /20 (homophones confondus) | WER (homophones confondus) |
|---|---|---|
| Vous allez moins vite que prévu, avec plus de pauses. Je propose la boucle courte, retour vers quinze heures et quart. | 0 (2) | 11,8 % (6,8 %) |
| Vous allez moins vite que prévu. Je propose la sortie la plus proche, retour vers quatorze heures et demie. | 7 (7) | 3,9 % |
| Vous faites plus de pauses que prévu. Je propose de garder la boucle, retour vers dix-sept heures moins le quart. | 0 (4) | 9,0 % (4,3 %) |
| Vous allez moins vite que prévu. Je propose la boucle courte, retour vers onze heures. | 10 | 3,7 % |
| Proposition acceptée. Dans trois cents mètres, continuez tout droit. | 18 | 3,3 % |

Erreurs : « Je propose » entendu « Je vous propose » ou « Propose » après la phrase de raison (la STT attend « je vous propose »), et « quinze heures et quart » entendu « quinze heures » (clip `loop.back.1515` existant, comme « midi et quart » plus haut). Même mesure avec « Je vous propose … » : 4/20, 18/20 et 20/20 annonces justes homophones confondus, WER 4,8 %, 0,8 % et 0 % (au lieu de 6,8 %, 3,9 % et 4,3 %) : appliqué le même jour (section suivante). Dans les packs, chaque clip seul : 7,3 % (4/55 : « pause » ×2, « Je vous propose », « bouc courte ») ; après « Toujours sur l'itinéraire. » : 2,5 % (2/79, seulement « pause ») ; échantillons assemblés `11-` à `15-` (`samples/`) : « Je vous propose » dans 2 sur 3, « retour vers 15h » pour « quinze heures et quart », le reste exact, `15-demi-tour-avance` exact.

**Voix publique et voix personnelle** : les noms viennent des couches construites (`skimap_voice::sources::site_names`) avec leur provenance. Un nom est public si une source publiable le donne (OSM et OpenSnowMap, refuges.info, INPN, geo.admin.ch), personnel sinon (Biodiv'sports, plan des pistes de station P22, source inconnue) ; mêmes sources reconnues par nom que le pack (`PERSONAL_SOURCES` = `UNAUTHORISED_SOURCES`, testé). La couche liste ces clips dans `voice/personal.json` (`PersonalClips` : ids et sources ; le manifeste lu par le téléphone ne change pas) ; `voice.meta.json` liste les sources personnelles marquées `personal_use_only`. Le pack sans `personal_sources` copie la voix sans ces clips ni ces sources (manifeste et métadonnées refaits, `personal.json` non copié) ; avec, il copie tout (une couche sans `personal.json` et avec une source personnelle reste écartée en entier). Test `public_pack_from_real_outputs_has_no_restricted_source` (pack public construit depuis `data/out`) : aucun nom ni date Biodiv'sports, aucun nom du plan absent d'OSM dans la voix publique ; il échoue si le filtre est retiré ou si la liste des clips personnels est ignorée (deux mutations essayées).

| | Cervières | Chapelle-des-Bois |
|---|---|---|
| Noms publics (pistes, lieux, dates) | 23, 23, 0 | 105, 100, 0 |
| Noms personnels | 5 lieux (espèces de Biodiv'sports : « Aigle royal »…), 4 dates | 3 pistes du plan (« Grande Combe », « La Victor Arbez », « Paradis »), 6 lieux (« APPB … », « Grand tétras - hiver », « Site RAMSAR … »), 1 date |
| Clips de la couche (fixes, valeurs, noms) | 899 (91, 757, 51), 3 761 137 o, 1 223 s | 1 141 (91, 836, 214), 4 677 989 o, 1 525 s |
| dont personnels | 9, 33 540 o | 10, 59 349 o |
| Voix du pack public (clips, octets avec manifeste et métadonnées) | 890, 4 067 678 o (clips 3 727 597 o) | 1 131, 5 049 876 o (clips 4 618 640 o) |
| Clips existants inchangés (graine, durée, taille) | 891 sur 892 (seul `fix.turnaround-late` change) | 1 130 sur 1 131 |
| Génération : GPU partagé avec un autre processus (88 % d'occupation, charge 60 à 77) ; puis GPU libre mais charge CPU 130 sur 20 cœurs | 62,8 s (TTS 55,0 s) ; 153,5 s (TTS 138,4 s) | 70,8 s (TTS 65,5 s) ; 174,7 s (TTS 157,7 s) |

Aucune attaque coupée ; les deux générations donnent les mêmes clips (graine, durée, taille). Les temps ne se comparent pas aux 26,8 et 34,2 s du matin : le poste était chargé par d'autres travaux (GPU puis CPU).


## Vouvoiement des propositions et « et quart » des heures (15/11/2026)

**« Je vous propose »** dans les trois phrases `fix.plan-propose-*` (« Je vous propose la boucle courte, », « … la sortie la plus proche, », « … de garder la boucle, »), et sur l'écran (`pace::proposal_text`) : la STT entendait déjà « Je vous propose » (attendu après « Vous allez… »), et l'app vouvoie partout. Ids inchangés.

**« quinze heures quinze » au lieu de « quinze heures et quart »** (`Clock::spoken`, clips `loop.back.HH15` des propositions et des résumés de boucle ; « et demie » et « moins le quart » inchangés ; les durées « une heure et quart » aussi : 20/20 justes sur 20 graines, seules et en fin de phrase). Le clip `loop.back.1515` était entendu « retour vers quinze heures » ; ce n'était pas une mauvaise graine : sur les graines 42 à 61, chaque texte seul, Parakeet v3, l'heure est juste (une heure dite = un mot, « 15h15 » = « quinze heures et quart » = « quinze heures quinze », `scoring3.py`) :

| Forme (16 heures de 6 h 15 à 21 h 15, ou 16 à 17 heures pour les autres) | Heure juste (20 graines) | WER, heure = un mot |
|---|---|---|
| « retour vers … heures et quart. » (avant) | **51 %** (162/320 ; 5 à 85 % selon l'heure) | 22,0 % |
| « retour prévu vers … heures et quart. » | 52 % (165/320) | 16,5 % |
| **« retour vers … heures quinze. »** (après) | **98 %** (313/320) | 4,9 % |
| « retour vers … heures et demie. » | 88 % (283/320) | 12,0 % |
| « retour vers … moins le quart. » | 92 % (294/320) | 14,0 % |

Erreur dominante avant : le quart disparaît (« Retour vers 15h. », 140 des 158 heures fausses). Clips des packs (graine 42), chaque clip seul : heures « et quart » justes 6/16 avant, « quinze » 16/16 après, sur les deux sites ; « et demie » 15/16, « moins le quart » 15/16 (inchangés).

**Propositions enchaînées** comme le téléphone (raison, 400 ms, « Je … propose …, », 150 ms, `loop.back.*`), graines 42 à 61, avant / après (homophones confondus : pause / pauses ; WER, heure = un mot) :

| Annonce | Avant (« Je propose », « et quart ») | Après (« Je vous propose », « quinze ») |
|---|---|---|
| … avec plus de pauses. … la boucle courte, retour vers 15 h 15. | 2/20 justes, 7,5 % | **16/20, 1,1 %** |
| … moins vite que prévu. … la sortie la plus proche, retour vers 14 h 30. | 7/20, 4,4 % | **18/20, 0,9 %** |
| … plus de pauses que prévu. … de garder la boucle, retour vers 16 h 45. | 4/20, 5,6 % | **20/20, 0 %** |
| … moins vite que prévu. … la boucle courte, retour vers 11 h. | 10/20, 3,9 % | **16/20, 1,7 %** |
| Les quatre | 3,9 % (98/2 520 mots) | **0,9 %** (12/1 360) |

(Comptées avec un mot par heure, les mesures du 15/11 au matin donnaient 6,8 %, 3,9 %, 4,3 % ; « Je vous propose » seul : 4,8 %, 0,8 %, 0 %.) Clips des packs refaits, enchaînés : les quatre annonces justes (homophones confondus) sur les deux sites.

**Couches voice refaites** (`voice_pack`, GPU libre, CPU chargé à 80 sur 20 cœurs : temps non comparables) : 19 clips changés par site (les 3 `fix.plan-propose-*` et les 16 `loop.back.HH15`), tous à la graine 42, aucune attaque coupée, les autres identiques (texte, graine, durée, taille). Cervières 899 clips, 3 759 462 o, 1 222,9 s, 86,1 s (TTS 74,5 s) ; Chapelle-des-Bois 1 145 clips (4 noms de pistes de plus que le 15/11 au matin, venus de la couche `pistes` reconstruite entre-temps : « Combe des Cives multi-activités », « Élodie Bourgois-Pin », « Le Griffon », « Le Pré d'Haut »), 4 694 947 o, 1 530,9 s, 76,4 s (TTS 69,8 s). Échantillons `11-` à `15-` refaits.

## Proposition de profil (X14, 15/11/2026)

**Phrase nouvelle** `fix.plan-profile-mismatch` : « Votre rythme ressemble à celui d'un autre profil. Voulez-vous le choisir ? » (une question, finie par « ? » : seule exception à la règle du point, testée). Couches voice refaites sur les deux sites : 900 clips à Cervières, 1 146 à Chapelle-des-Bois, le clip nouveau à la graine 42 (3,37 s, 28 jetons, 9 744 o), aucune attaque coupée. Transcription (Parakeet v3) du clip de chaque site : exacte (WER 0/24).

