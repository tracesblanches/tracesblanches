# Voix : vouvoiement des propositions et « et quart » des heures

[Index de la voix](../voix.md)

## Vouvoiement des propositions et « et quart » des heures (15/11/2026)

Mesures faites sur la voix de synthèse pré-générée de l'époque (retirée le 21/11/2026, les annonces sont dites par la synthèse du téléphone) ; les textes choisis restent ceux de la banque.

**« Je vous propose »** dans les trois phrases `fix.plan-propose-*` (« Je vous propose la boucle courte, », « … la sortie la plus proche, », « … de garder la boucle, »), et sur l'écran (`pace::proposal_text`) : la STT entendait déjà « Je vous propose » (attendu après « Vous allez… »), et l'app vouvoie partout. Ids inchangés.

**« quinze heures quinze » au lieu de « quinze heures et quart »** (`Clock::spoken`, clips `loop.back.HH15` des propositions et des résumés de boucle ; « et demie » et « moins le quart » inchangés ; les durées « une heure et quart » aussi : 20/20 justes sur 20 graines, seules et en fin de phrase). Le clip `loop.back.1515` était entendu « retour vers quinze heures » ; ce n'était pas une mauvaise graine : sur les graines 42 à 61, chaque texte seul, Parakeet v3, l'heure est juste (une heure dite = un mot, « 15h15 » = « quinze heures et quart » = « quinze heures quinze », `scoring3.py`) :

| Forme (16 heures de 6 h 15 à 21 h 15, ou 16 à 17 heures pour les autres) | Heure juste (20 graines) | WER, heure = un mot |
|---|---|---|
| « retour vers … heures et quart. » (avant) | **51 %** (162/320 ; 5 à 85 % selon l'heure) | 22,0 % |
| « retour prévu vers … heures et quart. » | 52 % (165/320) | 16,5 % |
| **« retour vers … heures quinze. »** (après) | **98 %** (313/320) | 4,9 % |
| « retour vers … heures et demie. » | 88 % (283/320) | 12,0 % |
| « retour vers … moins le quart. » | 92 % (294/320) | 14,0 % |

Erreur dominante avant : le quart disparaît (« Retour vers 15h. », 140 des 158 heures fausses).

**Propositions enchaînées** comme le téléphone (raison, 400 ms, « Je … propose …, », 150 ms, `loop.back.*`), graines 42 à 61, avant / après (homophones confondus : pause / pauses ; WER, heure = un mot) :

| Annonce | Avant (« Je propose », « et quart ») | Après (« Je vous propose », « quinze ») |
|---|---|---|
| … avec plus de pauses. … la boucle courte, retour vers 15 h 15. | 2/20 justes, 7,5 % | **16/20, 1,1 %** |
| … moins vite que prévu. … la sortie la plus proche, retour vers 14 h 30. | 7/20, 4,4 % | **18/20, 0,9 %** |
| … plus de pauses que prévu. … de garder la boucle, retour vers 16 h 45. | 4/20, 5,6 % | **20/20, 0 %** |
| … moins vite que prévu. … la boucle courte, retour vers 11 h. | 10/20, 3,9 % | **16/20, 1,7 %** |
| Les quatre | 3,9 % (98/2 520 mots) | **0,9 %** (12/1 360) |

(Comptées avec un mot par heure, les mesures du 15/11 au matin donnaient 6,8 %, 3,9 %, 4,3 % ; « Je vous propose » seul : 4,8 %, 0,8 %, 0 %.)

