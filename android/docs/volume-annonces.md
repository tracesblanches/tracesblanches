# Volume des annonces (#179)

Cas de référence : volume Multimédia habituel d'environ 25 % (6 ou 7 sur 25 sur le haut-parleur d'un Pixel), soit environ −36 dB sur la courbe de volume du haut-parleur de la plateforme (AOSP : −40,8 dB à 20 %, −17 dB à 60 %, 0 dB à 100 %). À ce niveau, même des clips à −10 LUFS sont « à peine audibles ». Trois étages, sans réglage ni bouton.

## 1. Clips à −10 LUFS (pipeline)

Parole compressée (3:1, seuil relatif à la sonie du clip), gain vers −10 LUFS, limiteur (crête vraie ≤ −1 dBTP sur le fichier décodé) : `docs/voix/banque-et-clips.md`, section Sonie des clips. +4 dB par rapport à −14 LUFS, +11 dB par rapport aux clips d'avant #179.

## 2. Volume du flux pendant l'annonce (`StreamBoost`)

Les annonces jouent sur le flux de guidage (usage `USAGE_ASSISTANCE_NAVIGATION_GUIDANCE`, le flux Multimédia sur un Pixel). Pendant une sortie seulement (service de suivi avec une sortie ouverte), si ce volume est sous le plancher :

- **Plancher** : 70 % du maximum sur le haut-parleur (18 sur 25), soit environ −12 dB sur la courbe AOSP : **24 dB de plus qu'à 25 %**. 40 % (10 sur 25) quand un casque, des écouteurs (filaires, USB, Bluetooth A2DP, SCO ou LE) ou une aide auditive sont branchés : jamais plus fort que cela dans les oreilles. Un volume déjà au plancher ou au-dessus n'est pas touché.
- **Quand** : relevé juste avant le premier son de la file (bip de virage, annonce), au moment où le focus audio est pris ; rendu juste après le dernier, quand il est rendu. Des sons qui se suivent (le bip d'un virage et ses paroles, deux annonces en file) ne relèvent le volume qu'une fois : la valeur rendue est toujours celle d'avant le premier, jamais une valeur relevée.
- **Restitution exacte** : le volume revient à sa valeur d'avant, à l'unité près. Si l'utilisateur a changé le volume pendant l'annonce (plus fort ou plus faible), sa nouvelle valeur est gardée.
- **App tuée pendant l'annonce** : la valeur d'avant et la valeur relevée sont écrites (synchrone) avant le relèvement. Au démarrage suivant (écran principal, ou service de suivi avant toute annonce), le volume est rendu s'il est encore à la valeur relevée ; sinon la valeur de l'utilisateur est gardée.
- **Jamais** : hors sortie (essai des bips dans la préparation), en mode Ne pas déranger (tout filtre d'interruption autre que « tout »), pour la musique. Pendant l'annonce la musique est atténuée par le système (focus `AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK`), puis retrouve son volume rendu.

## 3. Gain de lecture (`LoudnessEnhancer`)

La voix Android de secours (texte lu quand un clip manque), non normalisée, lit sur sa propre session audio (`KEY_PARAM_SESSION_ID`) avec un `LoudnessEnhancer` de **600 mB (+6 dB)**, libéré après la lecture. Mesure par un portage de l'effet AOSP (`frameworks/av/media/libeffects/loudness`, compresseur 7:1 au-dessus de −8 dBFS, attaque 1 ms, relâchement 15 ms, écrêtage dur à pleine échelle ; `crates/voice/examples/relevel.rs`) sur des paroles non normalisées (clips d'avant #179, −21 LUFS) : +5,1 dB de sonie en médiane, aucun échantillon écrêté sur 2 046 clips ; à 900 mB seulement +6,4 dB, le compresseur travaille davantage.

Les clips n'en ont pas : sur des clips à −10 LUFS et −1 dBTP, l'effet **baisse** la sonie (compresseur au-dessus de −8 dBFS) : −3,0 dB en médiane à 0 mB, −1,3 dB à 600 mB avec 283 à 402 échantillons écrêtés (dépassement jusqu'à 3,2 dB) par site. Le niveau des clips est fait au poste, avec anticipation, sans écrêtage.

## Gain total au cas de référence

À 7 sur 25 sur le haut-parleur : clip −14 → −10 LUFS (+4 dB), volume 7 → 18 sur 25 (+24 dB sur la courbe AOSP) : environ **+28 dB** par rapport aux annonces jugées « à peine audibles ». Courbe réelle du Pixel non mesurée ici (pas d'essai sur le téléphone).
