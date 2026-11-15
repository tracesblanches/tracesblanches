# skimap-voice

Voix : banque de phrases françaises et clips pré-générés pour les packs. Tickets : VO2 (#84), VO3 (#85). Documentation : [docs/voix.md](../../docs/voix.md).

- Téléphone : `bank` (événement → annonce), `clips`, `manifest`, `names`, `numbers`, `quantity`.
- Poste : `pack::build_site` / `pack::build` (couche `data/out/<site>/voice/`, clips des noms des sources personnelles listés dans `personal.json`), `sources` (noms des couches construites et leur provenance), `tts` (pockettts.cpp), `audio` (coupe, niveau, Ogg/Opus), `tokenizer` et `gguf` (comptage exact des tokens).
- `scripts/build-pockettts.sh` : build CUDA de pockettts.cpp avec les patchs de speak.
- `examples/voice_pack.rs` : les deux sites, noms des couches construites (`--names` : noms seuls), 15 échantillons par site (noms publics seulement).
