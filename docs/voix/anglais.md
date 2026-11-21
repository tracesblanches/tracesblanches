# Voix anglaise (VO6 #129)

[Index de la voix](../voix.md)

**Banque de phrases anglaise.** Mêmes identifiants qu'en français (`turn.left.200`, `fix.arrive`, `piste.greffier.bleue`) ; chaque phrase est relue avec ses valeurs (`skimap_voice::phrase::Phrase::parse`) puis dite dans l'autre langue (`Phrase::clip`). Mots anglais dans `crates/voice/src/en.rs` (nombres, distances en mètres et kilomètres, durées, heures sur 12 heures avec le moment de la journée). Les noms propres restent tels quels.

**Téléphone.** Les annonces sont dites dans la langue de l'app (français sur un téléphone en français, anglais sinon), sans réglage, par la synthèse vocale du téléphone (voix `fr-FR` ou `en-GB` du moteur installé, hors ligne). Au démarrage d'une sortie, s'il n'y a aucune voix de la langue utilisable hors ligne, un message le dit à l'écran (« Aucune voix hors ligne pour les annonces… ») ; les annonces de sécurité vibrent toujours.

**Paquets de voix retirés (21/11/2026).** Les paquets de voix par site et par langue (clips Ogg/Opus, 4,1 à 5,3 Mo par site et par langue) ne sont plus construits ni publiés ; le catalogue du serveur n'a plus de champ `voice` (un champ `voice` d'un ancien serveur est ignoré) ; au démarrage de l'app, les paquets déjà installés (`voice/` à côté de `packs/`) sont supprimés et un téléchargement de paquet de voix encore en file est abandonné (`LegacyVoice`).
