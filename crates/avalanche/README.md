# skimap-avalanche

Exposition aux avalanches : oracle auto-ATES, réimplémentation Rust, validation CLPA / EPA (pipeline). Tickets : P16 (#29), P17 (#30), P18 (#31), P20 (#99).

Sommaire :

1. [Avalanches historiques et tronçons exposés (P20, module `historic`)](docs/historic.md)
2. [Exposition aux avalanches : couche, méthode, oracle (P16, P17, module `exposure`)](docs/exposure.md)
   - [Méthode](docs/exposure.md#méthode)
   - [Oracle (P16)](docs/exposure.md#oracle-p16--limplémentation-de-référence)
3. [Résultats : accord avec la référence, temps, couche utilisateur](docs/exposure-results.md)
4. [Validation contre la CLPA et l'EPA (P18), limites](docs/exposure-validation.md)

En bref : la couche `avalanche-exposure` (`exposure_5m.tif`, codes 1 « terrain peu exposé (modèle) », 2 « exposé », 3 « très exposé ») reproduit à 100 % des cellules l'implémentation de référence AutoATES v3.0 sur les deux jeux de test (alpin et jurassien), 7,6 fois plus vite ; le modèle seul laisse 1,68 % de la surface CLPA du jeu de test alpin en « peu exposé » (objectif 0, non atteint), d'où la fusion des emprises CLPA dans la couche. Jamais « sûr » ni « sans danger ».
