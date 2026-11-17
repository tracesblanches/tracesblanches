# Voix : propositions de C11, demi-tour avancé, voix publique

[Index de la voix](../voix.md)

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

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Noms publics (pistes, lieux, dates) | 23, 23, 0 | 105, 100, 0 |
| Noms personnels | 5 lieux (espèces de Biodiv'sports : « Aigle royal »…), 4 dates | 3 pistes du plan (« Grande Combe », « La Victor Arbez », « Paradis »), 6 lieux (« APPB … », « Grand tétras - hiver », « Site RAMSAR … »), 1 date |
| Clips de la couche (fixes, valeurs, noms) | 899 (91, 757, 51), 3 761 137 o, 1 223 s | 1 141 (91, 836, 214), 4 677 989 o, 1 525 s |
| dont personnels | 9, 33 540 o | 10, 59 349 o |
| Voix du pack public (clips, octets avec manifeste et métadonnées) | 890, 4 067 678 o (clips 3 727 597 o) | 1 131, 5 049 876 o (clips 4 618 640 o) |
| Clips existants inchangés (graine, durée, taille) | 891 sur 892 (seul `fix.turnaround-late` change) | 1 130 sur 1 131 |
| Génération : GPU partagé avec un autre processus (88 % d'occupation, charge 60 à 77) ; puis GPU libre mais charge CPU 130 sur 20 cœurs | 62,8 s (TTS 55,0 s) ; 153,5 s (TTS 138,4 s) | 70,8 s (TTS 65,5 s) ; 174,7 s (TTS 157,7 s) |

Aucune attaque coupée ; les deux générations donnent les mêmes clips (graine, durée, taille). Les temps ne se comparent pas aux 26,8 et 34,2 s du matin : le poste était chargé par d'autres travaux (GPU puis CPU).
