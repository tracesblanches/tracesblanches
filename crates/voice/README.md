# skimap-voice

Voix : phrases françaises et anglaises des annonces, lues par la synthèse vocale du téléphone (Android `TextToSpeech`, hors ligne). Tickets : VO2 (#84), VO6 (#129). Documentation : [docs/voix.md](../../docs/voix.md).

- `bank` : événement du cœur → annonce (phrases et pauses) ; `Announcement::text` est le texte lu par le téléphone.
- `clips` : chaque phrase avec ses valeurs (id stable `turn.left.200`, `fix.arrive`, et texte).
- `phrase`, `en` : la même phrase en anglais (même id).
- `names`, `numbers`, `quantity` : noms dits, nombres en toutes lettres, arrondis naturels.
- `vocabulary` : le vocabulaire fini sur les grilles d'arrondi (tests des deux langues).
