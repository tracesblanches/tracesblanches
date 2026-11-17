# Voix anglaise et paquets de voix par langue (VO6 #129, 17/11/2026)

**Banque de phrases anglaise.** Mêmes identifiants de clips qu'en français (`turn.left.200`, `fix.arrive`, `piste.greffier.bleue`) ; chaque clip est relu en phrase avec ses valeurs (`skimap_voice::phrase::Phrase::parse`) puis dit dans l'autre langue (`Phrase::clip`). Mots anglais dans `crates/voice/src/en.rs` (nombres, distances en mètres et kilomètres, durées, heures sur 12 heures avec le moment de la journée : « Four thirty in the afternoon. », retour « back around a quarter past twelve. ») et texte anglais de chaque phrase fixe à côté du français (`fixed_phrases!`). Niveaux du BERA en anglais EAWS (low, moderate, considerable, high, very high). Les annonces restent construites en français par la banque et le plan ; le téléphone les dit dans la langue des clips installés (`Announcement::in_lang`, `crates/ffi/src/tracking.rs`). Test : chaque clip d'un pack se relit en phrase, le français revient à l'identique, l'anglais a ses propres mots (sauf « Klister » et les noms).

**Noms propres français.** Lus par le modèle anglais, ils sont anglicisés. Transcription Parakeet-TDT-0.6B-v3 (sherpa-onnx 1.13.8, poste) des clips de noms publics, graine 42, WER sur les mots :

| Lecture des noms | Cervières (51 clips) | Chapelle-des-Bois (≈ 210 clips) |
|---|---|---|
| Modèle anglais, voix `estelle` | 76,9 % | 65,0 % |
| Modèle anglais, voix `alba` | 54,4 % | 44,3 % |
| **Paquet anglais retenu** : nom dit par le modèle français (`estelle`), puis « Blue trail. » en anglais | **50,3 %** | **37,8 %** |
| Référence : paquet français (`voice/`) | 43,7 % | 30,9 % |

Le paquet anglais dit donc le nom comme la voix française (même locutrice), puis la couleur en anglais, 150 ms de silence entre les deux, dans un seul clip (`skimap_voice::pack::clip_parts`). L'écart restant avec le paquet français vient surtout du transcripteur, qui bascule en anglais sur la fin du clip (« Izoard. » entendu « Is it? »).

**Clips anglais du paquet** (Cervières, chacun transcrit seul) : phrases fixes 9,9 % (50/506 mots), valeurs 12,4 % (451/3 634) ; une part des écarts est d'orthographe (« kilometer » pour « kilometre », « Turn around time »). Corrigé après mesure : « A eleven kilometre loop » → « An eleven kilometre loop ».

**Paquets de voix.** Les clips sortent du pack de base (`BuildOptions::voice` désactivé par défaut) ; `skimap build-pack <site>` (ou `skimap voice-pack <site>`) écrit un paquet par langue dont la couche est construite (`voice/` → `data/out/<site>/voice-pack/fr/`, `voice-en/` → `voice-pack/en/`, `skimap voice <site> --lang en`). Le serveur publie chaque paquet sous `/v1/sites/<site>/voice/<lang>/index.json`, le catalogue le liste dans le champ `voice` du site ; son paquet de base est refait à chaque publication : un premier téléchargement ou un changement de langue tient en un seul paquet.

Tailles mesurées le 17/11/2026, après le rebasage sur main (phrases « Ramène-moi » comprises : 97 phrases fixes) ; voix française et anglaise régénérées hors du dossier de données partagé, paquets publiés dans un magasin de test avec les packs de site du 15/11. Pack de base sans voix : pack du 15/11 moins ses fichiers `voice/` (le pack n'a pas été reconstruit, ses fonds demandent les téléchargements IGN et Protomaps) :

| Site | Pack de base sans voix | Paquet FR | Paquet EN | Téléchargement FR / EN (un paquet) |
|---|---|---|---|---|
| Cervières | 42 251 452 o | 4 097 639 o | 4 280 179 o | 4 113 386 o / 4 203 290 o |
| Chapelle-des-Bois | 37 528 435 o | 5 079 838 o | 5 270 664 o | 5 142 197 o / 5 240 299 o |

Génération anglaise sur le poste (RTX 5060 Ti) : 23,9 s (Cervières) et 28,9 s (Chapelle-des-Bois), aucune attaque coupée, 27 et 37 jetons au plus.

**Téléphone.** La langue suit celle du téléphone (anglais par défaut, français sur un téléphone en français), sans réglage. Au démarrage de l'app et à la mise à jour quotidienne, le paquet de la langue du téléphone est mis en file s'il manque (`PackSync.followLanguage`, un téléchargement) ; hors ligne il attend le réseau et la voix déjà installée reste et parle (`voice_choice`) ; une fois la nouvelle installée, l'autre est supprimée. Un clip absent : la voix Android lit le texte dans la langue des annonces (`TrackingSession.speechLang`, `Announcer.language`).
