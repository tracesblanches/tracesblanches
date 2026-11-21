# Volume des annonces (#179)

Décision du 21/11/2026 : **l'app ne modifie jamais le volume** d'aucun flux. Le niveau des annonces est celui de la synthèse vocale du téléphone, relevé par un effet sur sa propre session audio, et le volume reste celui que l'utilisateur a choisi.

## Flux, focus, effet

- Les annonces et les bips (K6) jouent sur le flux de guidage : usage `USAGE_ASSISTANCE_NAVIGATION_GUIDANCE`, contenu `CONTENT_TYPE_SPEECH` (le flux Multimédia sur un Pixel), à pleine échelle du lecteur (`KEY_PARAM_VOLUME` 1).
- Focus transitoire `AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK` : la musique est atténuée par le système du premier son de la file au dernier, puis retrouve son volume.
- Enchaînement : les annonces qui se suivent vont tout de suite dans la file du moteur (`QUEUE_ADD`), le moteur les dit l'une après l'autre sans attendre l'app ; un bip attend les paroles mises en file avant lui, les paroles d'après attendent le bip.
- `LoudnessEnhancer` de **600 mB (+6 dB)** sur la session audio de la synthèse (`KEY_PARAM_SESSION_ID`), créé au démarrage du moteur, libéré à la fermeture. Mesure (21/11/2026) par un portage de l'effet AOSP (`frameworks/av/media/libeffects/loudness` : gain en entrée, compresseur 7:1 au-dessus de −8 dBFS, attaque 1 ms, relâchement 15 ms, écrêtage dur à pleine échelle) sur les voix du moteur du téléphone : 12 annonces de la banque (6 en français, 6 en anglais) synthétisées en WAV par Google Synthèse vocale (voix hors ligne `fr-FR` et `en-GB`, émulateur Android 15, `SpeechDeviceTest#announcementsToWav`), −18,3 LUFS en médiane, crêtes de −2,3 à −4,3 dBFS, sonie mesurée par `ffmpeg ebur128`. À 600 mB : **+3,4 dB** de sonie en médiane (+2,7 à +4,0), **aucun échantillon écrêté** ; à 900 mB : +4,4 dB mais 2 échantillons écrêtés (+0,9 dB au-dessus de la pleine échelle) sur une annonce anglaise. 600 mB est gardé. À refaire sur le téléphone avec sa voix installée.

## Annonces périmées (#183)

Le moteur tient l'annonce qu'il dit et la suivante (enchaînement sans trou) ; les autres attendent leur tour dans l'app et sont vérifiées au moment d'être données au moteur (`Freshness`) :

- le même texte donné au moteur il y a moins de **30 s** (horloge du téléphone) n'est pas redit, annonces de sécurité comprises (elles ont déjà vibré) ;
- une annonce du cœur faite à plus de **50 m** de la dernière position connue est périmée et n'est pas lue (un quart des 200 m d'un virage annoncé, environ 4 s de descente rapide), sauf une annonce de sécurité, dite même en retard ;
- ce qu'un bouton ou une extension demande est toujours dit.

C'est ce qui empêchait un rejeu accéléré (3 h 30 de sortie en 24 s) d'empiler toutes les annonces de la sortie et de les lire l'une après l'autre pendant des minutes.

## Volume bas : un avertissement, rien de changé

Au démarrage d'une sortie, si le volume du flux des annonces est sous **un tiers de son maximum** (`SpeechChecks.LOW_VOLUME_FRACTION`), l'app affiche « Volume bas : les annonces risquent de ne pas s'entendre » (« Low volume: the announcements may not be heard »), sans rien changer. Seuil : sur la courbe du haut-parleur AOSP (−40,8 dB à 20 %, −17 dB à 60 %, 0 dB à 100 %), un tiers vaut environ −30 dB ; le cas signalé « à peine audible » (6 ou 7 sur 25 sur un Pixel, environ 25 %, −36 dB) est dessous, la moitié du maximum (environ −22 dB) ne l'est pas. Un flux coupé (0) est bas.

## Vibration des annonces de sécurité

Chaque annonce de sécurité (priorité `Safety` du cœur, alertes des extensions) vibre en plus d'être dite : le motif de son alerte quand elle en a un, sinon trois longues impulsions (`HapticPatterns.SAFETY`, usage alarme : senties même en mode vibreur ou silencieux). Sans voix hors ligne, elles restent senties.

## Versions précédentes

Le relèvement du volume pendant les annonces (`StreamBoost`, 21/11/2026) est retiré : la valeur d'avant qu'il gardait pour la rendre (`skimap_stream_boost`) est supprimée au démarrage de l'app, sans être appliquée.
