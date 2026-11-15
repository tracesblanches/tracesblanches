# Règles des signalements (V9 #72, V10 #73, V11 #74)

[← skimap-collab](../README.md)

Règles codées dans `src/rules.rs` (fonctions pures du temps, testées avec une horloge simulée) et appliquées par `src/store.rs`. Le serveur les publie aussi : `GET /v1/collab/rules`.

## Types et durées de vie

| Type (`kind`) | Libellé | Durée de vie sans confirmation |
|---|---|---|
| `track` | trace faite | 48 h |
| `groomed` | piste damée | 24 h |
| `ice` | glace | 24 h |
| `wind_blown` | neige soufflée | 24 h |
| `obstacle` | obstacle | 7 jours |
| `closed` | piste fermée sur le terrain | 3 jours |

La durée part de l'heure du constat (le geste), pas de l'envoi.

## Confirmation, réfutation, réputation

1. « Toujours là » par un autre appareil à l'heure `t` repousse l'expiration à `t + durée de vie` si c'est plus tard, sans dépasser `constat + 4 × durée de vie`.
2. « Plus là » pèse contre. Poids pour : l'auteur et chaque confirmation ; poids contre : chaque réfutation ; chaque vote pèse la réputation du votant bornée à [0,5 ; 2]. Le signalement est réfuté (caché) quand le poids contre atteint le poids pour + 0,5 : il faut deux « plus là » contre un signalement seul, trois s'il a été confirmé une fois.
3. Un vote par appareil et par signalement (un nouveau vote remplace l'ancien) ; pas de vote sur son propre signalement.
4. Réputation d'un appareil : 1 au départ ; +0,1 par confirmation d'un de ses signalements par un autre (au plus 2) ; −0,3 par signalement réfuté (au moins 0). Sous 0,5, ses nouveaux signalements sont retenus : invisibles tant qu'un autre appareil ne les a pas confirmés.
5. L'identifiant d'appareil tourne tous les 30 jours (choix de confidentialité) : la réputation repart alors de 1. Un tricheur qui change d'identifiant retrouve 1, pas plus ; les limites par appareil et par adresse bornent ce qu'il peut faire.

## File hors réseau

Le téléphone garde les signalements faits sans réseau et les envoie au retour du réseau (WorkManager), avec l'heure réelle du constat. Le serveur accepte un constat vieux de 72 h au plus et en avance de 5 min au plus (horloge du téléphone) ; un signalement déjà expiré à son arrivée (glace vue il y a 25 h) reçoit 410 et le téléphone le retire de sa file sans le stocker.

## Modération et abus

- **Pas de texte libre** dans les signalements : personne ne relit ce que le serveur publierait (injures, données personnelles, numéros de téléphone). Les six types couvrent le ticket ; un détail se dit par une note OSM, que l'utilisateur relit et envoie lui-même à OpenStreetMap, modérée par sa communauté.
- Photo facultative : JPEG de 60 Ko et 1280 px au plus, réduite par le téléphone, métadonnées retirées par le serveur ; montrée seulement après une confirmation par un autre appareil ; un seul signalement de la photo (« photo inappropriée ») la supprime aussitôt.
- Limites : par appareil, 12 signalements par heure et 40 par jour, 60 votes par heure, 10 photos par jour ; par adresse (en mémoire, jamais stockée), 240 écritures par heure ; pour tout le service, 20 000 signalements par jour.
- Purge horaire : signalements expirés ou réfutés, leurs votes et leur photo ; appareils non revus depuis 60 jours.
- Position limitée à la France métropolitaine et ses frontières, Corse comprise.
