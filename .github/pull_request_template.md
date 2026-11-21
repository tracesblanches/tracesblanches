## Ce qui change

<!-- Le problème, puis ce que la demande de fusion change. Citez l'issue : Refs #… -->

## Preuves

<!-- Collez la sortie des commandes lancées et les chiffres avant et après. -->

- [ ] `scripts/ci --quick` passe (fmt, clippy, tests) ; sortie collée ci-dessus.
- [ ] Un test couvre chaque comportement modifié ; il échoue sans le changement.
- [ ] Les références dorées touchées (`fixtures/golden/`) sont mises à jour dans le même commit, avec l'écart expliqué.
- [ ] La documentation est à jour (`README.md`, `docs/`, README de la crate).
- [ ] J'ai relu et je comprends chaque ligne, qu'un outil d'IA l'ait écrite ou non.

## Données

- [ ] Aucune source à usage personnel n'entre dans un pack publié ni dans `fixtures/`, `benches/data/` ou `fuzz/`.
- [ ] Aucun plan des pistes de station n'est redistribué (ni image, ni raster, ni plan déformé).
- [ ] Toute nouvelle source a sa licence citée dans `docs/licences/` et son attribution dans les métadonnées de sa couche.

## Accord du contributeur

- [ ] Chaque commit porte une ligne `Signed-off-by` (`git commit -s`), selon le [Developer Certificate of Origin 1.1](https://developercertificate.org/).
- [ ] J'accepte que ma contribution soit diffusée sous GPL-3.0-or-later avec la permission additionnelle de LICENSE-EXCEPTION (sous EUPL-1.2 pour `crates/avalanche`), et j'en détiens les droits.
