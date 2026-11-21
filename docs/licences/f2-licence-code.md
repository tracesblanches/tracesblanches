# F2 : licence du code, code et modèles tiers, licence des données produites

Ticket #2. Retour au [sommaire](../licences-donnees.md). Vérifié le 15/11/2026 (API GitHub, fichiers de licence, `cargo metadata`).

## Code et modèles tiers repris

| Élément | Licence | Ce que le dépôt en fait | Obligation |
|---|---|---|---|
| com4FlowPy (AvaFrame, `OpenNHM/AvaFrame`) | EUPL-1.2 (`LICENSE.txt` ; README : « Licensed with European Public License EUPL ») | `crates/avalanche/src/exposure/flowpy.rs` est un portage qui suit « the Numba kernel operation for operation » (`flowCoreNumba.py`) : à traiter comme œuvre dérivée | diffusion sous EUPL-1.2 (ou licence compatible, voir plus bas), avis d'origine et de modification, code source accessible |
| Flow-Py (`avaframe/FlowPy`, archivé) | GPL-3.0 | non repris directement ; AvaFrame dit que com4FlowPy « is now the actively maintained version of Flow-Py » | le passage GPL-3.0 → EUPL est le fait des auteurs (BFW, mêmes personnes dans les deux projets) ; accord de tous les contributeurs : **non vérifié** |
| AutoATES v3.0 (`AutoATES/autoATES-v3.0-issw`) | MIT, « Copyright (c) 2024 AutoATES » | zones de départ, classes dev8, paramètres portés en Rust ; oracle Python hors dépôt | garder l'avis de copyright et le texte MIT |
| snowtools (`UMR-CNRM/snowtools`) | CeCILL-C v1 (`LICENCE.txt`) | ni code ni fichier dans le dépôt : `massifs.shp` téléchargé au commit `c84a1609` ; seul `snow/massif.tif` en dérive | CeCILL-C 5.3.3 : « ce Logiciel Dérivé peut être distribué sous un contrat de licence autre que le présent Contrat à condition de respecter les obligations de mention des droits » (art. 6.4) : citer snowtools, CNRM et la licence |
| `vendor/pmtiles` 0.28.1 | « MIT OR Apache-2.0 » (`Cargo.toml`, `LICENSE-MIT`, `LICENSE-APACHE` ; amont `stadiamaps/pmtiles-rs`) | copie corrigée (`src/writer/mod.rs`) | choisir MIT (avis à garder) ; sous Apache-2.0, le fichier modifié devrait porter un avis de modification (absent aujourd'hui, seulement dans `VENDOR.md`) |
| 320 paquets tiers de `cargo metadata` (toutes cibles) | tous permissifs (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CDLA-Permissive-2.0, bzip2-1.0.6) ; une seule propose aussi LGPL en option | dépendances | avis de licence dans l'app (écran « Licences ») |
| Pocket TTS, modèle (`kyutai/pocket-tts-without-voice-cloning`) | CC-BY-4.0, plus « Prohibited use » (pas de clonage sans consentement, pas de tromperie) (https://huggingface.co/kyutai/pocket-tts-without-voice-cloning) | clips générés sur le poste ; modèle embarqué seulement en option (VO4) | si le modèle est livré : attribution Kyutai, lien CC BY 4.0, mention des modifications (conversion GGUF, Q8_0) |
| Voix `estelle` | CC0 : `unmute-prod-website/developpeuse-3.wav`, « The others are our own recordings and you may use them as CC0 » (https://huggingface.co/kyutai/tts-voices) | voix par défaut | aucune ; crédit de courtoisie |
| pockettts.cpp (`ServeurpersoCom/pockettts.cpp`) | MIT, « Copyright (c) 2023-2026 The pockettts.cpp authors » | outil du poste (et du téléphone en VO4) | avis MIT si livré |
| ggml (`ggml-org/ggml`), pocket-tts (`kyutai-labs/pocket-tts`) | MIT | idem | avis MIT si livré |

## EUPL-1.2 : ce qu'elle impose et ce qu'elle permet

Texte : https://interoperable-europe.ec.europa.eu/collection/eupl/eupl-text-eupl-12 (version 1.2, JO du 19/05/2017, 23 langues dont le français).
- Clause copyleft : la diffusion d'œuvres dérivées « will be done under the terms of this Licence or of a later version of this Licence » ; aucune condition supplémentaire.
- Clause de compatibilité : une œuvre dérivée fondée à la fois sur l'œuvre EUPL et sur une œuvre sous licence compatible peut être diffusée sous cette licence compatible. Annexe : GPL v2 et v3, AGPL v3, OSL 2.1 et 3.0, EPL 1.0, CeCILL 2.0 et 2.1, MPL 2, LGPL 2.1 et 3, CC BY-SA 3.0 (hors logiciel), EUPL 1.1 et 1.2, LiLiQ-R et R+.
- « Distribution » inclut le fait de « providing access to its essential functionalities » (service en ligne compris) ; article 3 : code source fourni ou dépôt indiqué « for as long as the Licensor continues to distribute ».
- Vendre l'app n'est pas interdit ; seule la licence du code est imposée.

## Options

1. **EUPL-1.2 pour tout le dépôt** (recommandée). Une seule licence, la seule que le portage de com4FlowPy accepte sans passer par la clause de compatibilité ; texte officiel en français, droit d'un État membre (art. 15) ; dépendances permissives intégrables ; vente une fois permise ; si un jour une brique GPL-3.0 s'ajoute, la combinaison peut sortir sous GPL-3.0 par la clause de compatibilité.
2. **MIT OR Apache-2.0 pour tout sauf `crates/avalanche` en EUPL-1.2.** Réutilisation plus libre des autres crates, mais l'app (APK) contient du code EUPL : son code source reste à publier, et deux régimes compliquent l'écran « Licences ». À prendre si l'on veut garder le reste de l'app fermé un jour.
3. **GPL-3.0-or-later.** Possible pour une combinaison avec du GPL (clause de compatibilité), plus contraignante sans gain ici.
4. **Réécriture du ruissellement depuis l'article (D'Amboise et al., GMD 2022, CC BY)** puis licence permissive : seules les idées sont reprises, mais c'est un nouveau développement et l'oracle à 100 % devrait être reprouvé.

## Recommandation

**EUPL-1.2** pour le code, avec :
- `LICENSE` (texte EUPL-1.2 en français), `license = "EUPL-1.2"` dans `[workspace.package]` (aujourd'hui « License still to be decided (F2, #2) », `Cargo.toml` ligne 12) ;
- `THIRD-PARTY.md` : avis AvaFrame/com4FlowPy (EUPL-1.2, commit `d0451ce`, « portage en Rust, modifié le [date] »), AutoATES v3.0 (MIT complet), pmtiles (MIT), Protomaps basemaps (BSD-3-Clause, pour `crates/pack/assets/protomaps/style-light-fr.json`), snowtools (CeCILL-C, données lues au commit `c84a1609`) ;
- en tête de `flowpy.rs` : origine, licence et date de modification (art. 5 de l'EUPL).

## Licence des données produites (section du README)

> Les packs et couches produits par le pipeline sont une collection de couches indépendantes ; chacune garde la licence de sa source, écrite dans `<couche>.meta.json` et dans `attributions[]` du manifeste :
> - ODbL 1.0 : couches tirées d'OpenStreetMap (pistes, domaines, graphe de routage, fond Protomaps) ; l'extrait OSM et le code du pipeline qui les transforme sont publiés (ODbL 4.6) ;
> - CC BY-SA 4.0 : refuges (refuges.info, Camptocamp) ;
> - Licence Ouverte 2.0 : couches calculées par skimap à partir de données sous Licence Ouverte (pentes, exposition aux avalanches, enneigement, zones sans réseau, zones protégées), avec la mention des sources d'origine ;
> - CC BY 4.0 : météo Open-Meteo ;
> - les sources réservées à l'usage personnel (ENJ, Nordic France, mairie, EPA, CLPA d'INRAE, API gratuite d'Open-Meteo dans une version payante) ne sont jamais dans un pack publié (`personal_sources` désactivé).

Le dépôt ne contient pas de données tierces sauf deux BERA de test (`crates/meteo/tests/fixtures/`, LO 2.0, à attribuer à Météo-France) : `research/raw/` et `/data` sont dans `.gitignore`.

## Décision du 15/11/2026 : GPL-3.0-or-later avec exception pour les boutiques d'applications

Choix de l'utilisateur, à la place de la recommandation EUPL-1.2 ci-dessus.
- **Code de skimap** : GPL-3.0-or-later, plus une permission additionnelle (GPLv3 section 7) qui autorise la diffusion par les boutiques d'applications (App Store d'Apple, Google Play, etc.) malgré leurs conditions d'usage. Google Play n'en a pas besoin ; l'App Store, oui.
- **Portée de l'exception** : seulement le code dont l'auteur du dépôt détient les droits. Toute contribution extérieure doit l'accepter explicitement (`CONTRIBUTING`, accord de contributeur).
- **`crates/avalanche`** (portage de com4FlowPy, EUPL-1.2) : reste sous EUPL-1.2. On ne peut pas y ajouter l'exception. Vérifié le 15/11/2026 : seule `crates/cli` (pipeline) en dépend, et aucune crate embarquée sur le téléphone (`common`, `core`, `pack`, `assist`, `voice`) ; l'app ne reçoit que la couche calculée, qui est une donnée et non du code.
- **Dépendances tierces** : toutes permissives, compatibles avec la GPLv3.
- **Données produites** : inchangé (section suivante) ; la licence du code ne s'applique pas aux packs.
- **À faire** : `LICENSE` (GPL-3.0), `LICENSE-EXCEPTION` (texte de l'exception), `license = "GPL-3.0-or-later"` dans `[workspace.package]` et `license = "EUPL-1.2"` dans `crates/avalanche`, en-têtes, `THIRD-PARTY.md`, règle cargo-deny qui interdit qu'une crate embarquée dépende de `crates/avalanche`.

## Mise en œuvre (21/11/2026)

- `LICENSE` : texte de la GPL-3.0 (identique octet pour octet à https://www.gnu.org/licenses/gpl-3.0.txt, SHA-256 `3972dc97…b36986`) ; `LICENSE-EXCEPTION` : la permission additionnelle (section 7) pour les boutiques d'applications, limitée au code dont l'auteur détient les droits et aux contributions qui l'acceptent (`CONTRIBUTING.md`) ; `LICENSES/EUPL-1.2.txt` : texte officiel anglais de l'EUPL-1.2 publié par la Commission (Interoperable Europe).
- `license = "GPL-3.0-or-later"` dans `[workspace.package]`, hérité par toutes les crates (`license.workspace = true`) et repris par `fuzz/Cargo.toml` ; `license = "EUPL-1.2"` dans `crates/avalanche/Cargo.toml`.
- Dépendants de `skimap-avalanche` (`cargo tree -i skimap-avalanche --all-features`) : `skimap-cli` et `skimap-srndiff`, ce dernier seulement avec sa fonction `pipeline`. `skimap-ffi` (l'app) dépend de `skimap-srndiff` sans cette fonction : `cargo tree -p skimap-ffi -e normal -i skimap-avalanche` répond « did not match any packages ». La note du 15/11 (« seule `crates/cli` en dépend ») est donc complétée par `srndiff` côté pipeline.
- Règles cargo-deny : `deny.toml` vérifie maintenant les crates du workspace (`private.ignore = false`, chaque crate doit déclarer sa licence, GPL-3.0-or-later ou EUPL-1.2 pour `skimap-avalanche` seulement) et n'autorise `skimap-avalanche` que sous `skimap-cli` et `skimap-srndiff` ; `deny-phone.toml` vérifie le graphe de `skimap-ffi` (fonctions par défaut, cibles Android, sans dépendances de développement) et y interdit `skimap-avalanche` sans exception. Commande : `cargo deny --locked --manifest-path crates/ffi/Cargo.toml --config deny-phone.toml check bans licenses`.
- `THIRD-PARTY.md` est généré par `docs/licences/third-party.sh` (`cargo metadata`, `cargo tree`, résolution Gradle de l'app et POM des bibliothèques) ; `--check` dit s'il est à jour.
- Pas encore fait : en-têtes SPDX dans les fichiers sources, avis d'origine et de modification en tête de `crates/avalanche/src/exposure/flowpy.rs` (article 5 de l'EUPL), écran « Licences » de l'app.
