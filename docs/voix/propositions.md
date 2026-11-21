# Voix : propositions de C11, demi-tour avancé

[Index de la voix](../voix.md)

## Propositions de C11, demi-tour avancé (15/11/2026)

Mesures faites sur la voix de synthèse pré-générée de l'époque (retirée le 21/11/2026, les annonces sont dites par la synthèse du téléphone) ; les textes choisis restent ceux de la banque.

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

**Enchaînement** « raison » + « Je propose …, » + `loop.back.<HHMM>` existant, comme le téléphone l'assemblait alors (phrases seules, silences coupés, 400 ms puis 150 ms), graines 42 à 61, heures de la STT réécrites (« 15h15 » = « quinze heures et quart ») :

| Annonce | Exactes /20 (homophones confondus) | WER (homophones confondus) |
|---|---|---|
| Vous allez moins vite que prévu, avec plus de pauses. Je propose la boucle courte, retour vers quinze heures et quart. | 0 (2) | 11,8 % (6,8 %) |
| Vous allez moins vite que prévu. Je propose la sortie la plus proche, retour vers quatorze heures et demie. | 7 (7) | 3,9 % |
| Vous faites plus de pauses que prévu. Je propose de garder la boucle, retour vers dix-sept heures moins le quart. | 0 (4) | 9,0 % (4,3 %) |
| Vous allez moins vite que prévu. Je propose la boucle courte, retour vers onze heures. | 10 | 3,7 % |
| Proposition acceptée. Dans trois cents mètres, continuez tout droit. | 18 | 3,3 % |

Erreurs : « Je propose » entendu « Je vous propose » ou « Propose » après la phrase de raison (la STT attend « je vous propose »), et « quinze heures et quart » entendu « quinze heures » (phrase `loop.back.1515`, comme « midi et quart » plus haut). Même mesure avec « Je vous propose … » : 4/20, 18/20 et 20/20 annonces justes homophones confondus, WER 4,8 %, 0,8 % et 0 % (au lieu de 6,8 %, 3,9 % et 4,3 %) : appliqué le même jour (section suivante). Dans les packs, chaque clip seul : 7,3 % (4/55 : « pause » ×2, « Je vous propose », « bouc courte ») ; après « Toujours sur l'itinéraire. » : 2,5 % (2/79, seulement « pause ») ; échantillons assemblés `11-` à `15-` (`samples/`) : « Je vous propose » dans 2 sur 3, « retour vers 15h » pour « quinze heures et quart », le reste exact, `15-demi-tour-avance` exact.
