# Voix de chaque site par le rattrapage (VO6 #129, 21/11/2026)

## Cause

Les paquets de voix n'étaient publiés que pour 2 sites sur 268 : la couche voix (`data/out/<site>/voice/`, `voice-en/`) ne se construisait qu'à la main (`skimap voice <site> --lang fr|en`, GPU du poste), jamais par `build-all` ni par le rattrapage ; l'image `pipeline` n'avait ni Pocket TTS, ni ses modèles, ni ffmpeg. `build-pack` ne fait un paquet de voix que pour une couche voix présente : 266 sites n'en avaient pas, et le champ `voice` du catalogue restait vide.

## Ce qui est fait

- **Étapes `voice` et `voice-en` de `build-all`** (donc du rattrapage `skimap-job catchup`), avant `build-pack` : la couche de chaque langue depuis les noms des pistes, refuges et zones sur le disque. Refaites quand ces couches changent (`watches`), sans être bloquées par leur échec (les noms d'avant, avec leur date, restent dits) ; reprises sinon. Le rafraîchissement seul des refuges (`skimap refresh`) les marque à refaire. Empreinte du code : phrases de la banque, façon de dire les noms, format du cache, modèles. Un site dont `build-all.json` n'a pas encore ces étapes est repris par le premier rattrapage (étapes manquantes), sans refaire les autres.
- **Cache des clips** `data/cache/voice/clips/<k[..2]>/<k>.opus|.json`, partagé par tous les sites et les deux langues : la clé `k` porte le texte de chaque partie, les empreintes du modèle et des voix, la voix, la graine, les réglages de coupe, de niveau, de débit et la pause, et la normalisation de sonie (#179 : cible -10 LUFS, plafond -1 dBTP, tolérance, compresseur, limiteur, version de l'algorithme `loudness::ALGORITHM_VERSION`, 2 depuis la compression). Format du cache 2 depuis #179 : un clip mis en cache avant, à la sonie des parties (≈ -21 LUFS), n'est jamais repris, ni sous son ancienne clé ni par sa description sans sonie. Une phrase inchangée n'est jamais resynthétisée ; les phrases fixes et les valeurs (≈ 830 clips par langue, les mêmes partout) ne le sont qu'une fois pour la France. Une synthèse à la fois sur le dossier de données (verrou `synthesis.lock`) : un seul processus Pocket TTS, quel que soit le nombre de sites construits en même temps.
- **Image `pipeline`** : pockettts.cpp compilé pour le CPU de la cible (`crates/voice/scripts/build-pockettts.sh`, `BACKEND=cpu`), modèles fr et en, ffmpeg ; voir [deploy/README.md](../../deploy/README.md).

## Mesures

Poste (RTX 5060 Ti, 20 cœurs), chargé par d'autres travaux (charge 65 à 127) pendant toutes les mesures. Couches lues dans une copie de travail des sorties réelles.

**35 sites, fr puis en, GPU** (dont Jura, Vosges, Pyrénées-Orientales, Ariège, Cantal, Loire, Aubrac, Vercors, Chartreuse, Aravis, Glières, Chablais, Haute-Maurienne, Briançonnais, Queyras, Mercantour, Morvan, Sancy) :

| | fr | en |
|---|---|---|
| 1er site (cache vide) : clips synthétisés | 1 031 en 106 s | 965 en 113 s |
| sites suivants : clips synthétisés (moyenne, médiane, max) | 47,6 ; 29 ; 224 | 20,6 ; 10 ; 175 |
| taille de la couche (moyenne, max) | 3,96 Mo ; 4,96 Mo | 4,16 Mo ; 5,19 Mo |
| mémoire maximale (skimap + pocket-tts CUDA) | 870 Mo | 861 Mo |

Un site sans phrase nouvelle ne lance pas Pocket TTS (17 Mo, 0,5 à 3,9 s : copie des clips depuis le cache).

**Image `pipeline` (amd64), CPU, `--cpus 3 --memory 4g --read-only`**, cache rempli au GPU (même clé : le périphérique n'en fait pas partie) : 25 et 9 clips en 19 et 6 s (Béarn), 58 et 12 clips en 29 et 6 s (Vosges), 19 et 11 clips en 16 et 8 s (Massif central). Pic mémoire du conteneur (`memory.peak`) : **648 Mio**. Temps CPU de pocket-tts CPU sur 40 noms de pistes : 21,0 s CPU sur 1 fil, 19,7 s sur 3 fils (0,5 s CPU par clip).

## Extrapolation à 268 sites

- Clips à synthétiser une fois pour la France : ≈ 2 000 (phrases fixes et valeurs, fr et en) + 268 × (47,6 + 20,6) ≈ 18 300 noms, soit ≈ 20 300 clips.
- Temps au premier passage, à 0,5 à 0,8 s par clip sur 3 CPU (mesuré ci-dessus, poste x86 chargé ; les cœurs arm64 du serveur ne sont pas mesurés) : **2,8 à 4,5 h** de synthèse, en série (verrou). Le rattrapage s'arrête de lancer des sites après 240 min et reprend 6 h plus tard : la voix de toute la France est faite en 1 à 2 passages, avec les autres étapes. Le site le plus lourd mesuré (224 + 175 clips) prend 3 à 5 min.
- Ensuite : seuls les noms changés sont synthétisés (quelques clips, quelques secondes) ; une voix inchangée est reprise sans rien lancer.
- Mémoire : un seul Pocket TTS à la fois, ≤ 648 Mio en plus de l'autre site construit en même temps ; d'après le relevé du 15/11 (7,3 Go pour 8 sites, 2 sites sous 4 Gio hors sections de la GTJ), 2 sites et la voix restent sous 4 Gio. Non mesuré sur un rattrapage réel sur le serveur.
- Disque : ≈ 8,1 Mo de couches voix par site, ≈ 2,2 Go pour 268 sites, autant pour les paquets de voix ; cache ≈ 20 000 clips (≈ 45 Mo sur disque pour 4 425 clips mesurés, ≈ 200 Mo pour la France). Téléchargement d'une langue sur le téléphone : ≈ 4 Mo par site.
