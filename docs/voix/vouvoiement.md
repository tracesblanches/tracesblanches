# Voix : vouvoiement des propositions et « et quart » des heures

[Index de la voix](../voix.md)

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
