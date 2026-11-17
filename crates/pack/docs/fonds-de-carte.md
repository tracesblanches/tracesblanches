# Fonds de carte par site (P4, #17)

[← skimap-pack](../README.md)

Deux fonds vectoriels par pack, chacun en PMTiles avec son style MapLibre, son sprite et ses glyphes, pour s'afficher sans réseau.

## Plan IGN v2 (`build::plan_ign`)

- Tuiles TMS `https://data.geopf.fr/tms/1.0.0/PLAN.IGN/{z}/{x}/{y}.pbf` de l'emprise du site, zooms 0 à 16, par le téléchargeur commun (source `ign`, 20 requêtes/s au plus). Les tuiles et le TileJSON sont figés par l'édition : une tuile du cache validée après le jour de la date `revision` de la fiche `IGNF_PLAN-IGN` sert quel que soit son âge, et n'est téléchargée de nouveau que quand la fiche annonce une édition plus récente (fiche relue au plus une fois par jour). Le TMS n'envoie ni `ETag` ni `Last-Modified` : avec une durée de cache, chaque tuile était retéléchargée à l'identique tous les 30 jours. Les corps arrivent en protobuf brut ; l'archive les stocke en gzip. Un 404 isolé (vu une fois sur 16/33983/23609, 200 aux trois essais suivants) est réessayé quatre fois, comme le WMTS des pentes.
- Style `standard.json` de l'IGN réécrit : source `pmtiles://basemap/plan-ign.pmtiles`, sprite `PlanIgn` (1x et 2x) et glyphes des cinq piles « Source Sans Pro » copiés dans le pack, plages 0-255, 256-511 et 8192-8447 (latin, latin étendu A, ponctuation).
- Licence Etalab 2.0 ; stockage hors ligne ni interdit ni autorisé expressément par les CGU (L2, #7) : usage personnel.
- Attribution (la Licence Ouverte demande la source et la date de dernière mise à jour) : « © IGN – Plan IGN v2, mise à jour du AAAA-MM-JJ (fiche IGNF_PLAN-IGN), tuiles téléchargées le AAAA-MM-JJ (ou du … au …), Licence Ouverte Etalab 2.0 ». La date de mise à jour est la date `revision` (sinon `publication`) de la citation de la fiche du catalogue `IGNF_PLAN-IGN` (CSW `GetRecordById`, lue par le téléchargeur : 2026-10-08 le 15/11/2026), les dates de téléchargement celles des tuiles dans le cache. Elle est dans `attributions[]` et `basemaps[].sources[]` (`attribution`, `edition`) du manifeste, `basemaps[].data_date`, le panneau « Attributions » du visualiseur et `LISEZMOI.md` de l'export. Les métadonnées de l'archive PMTiles portent la même mention sans les dates de téléchargement (« © IGN – Plan IGN v2, mise à jour du AAAA-MM-JJ (fiche IGNF_PLAN-IGN), Licence Ouverte Etalab 2.0 ») : l'archive ne dépend que des tuiles, de l'édition et du TileJSON, si bien que deux constructions sans nouvelle édition IGN donnent la même archive à l'octet près (X26, #160 ; test `two_writes_of_the_same_tiles_are_identical`). Le 15/11/2026, la seule date de téléchargement dans l'archive avait fait publier toute l'archive (11 Mo) dans une mise à jour différentielle. Le coin de la carte garde « © IGN Plan IGN v2 ».

## OpenStreetMap, Protomaps (`build::osm`)

- Extrait du build quotidien `https://build.protomaps.com/AAAAMMJJ.pmtiles` (139 Go, liste `build-metadata.protomaps.dev/builds.json` lue par le téléchargeur, source `protomaps`), emprise du site, zooms 0 à 15 (le maximum des builds).
- L'extrait se fait en Rust par requêtes partielles du téléchargeur commun (`build::extract` : en-tête et répertoire racine dans les 16 premiers Kio, répertoires feuilles des seules tuiles de l'emprise, données lues en plages fusionnées quand l'écart est d'au plus 256 Kio ; chaque plage est une entrée du cache). Build 20261114 : Jeu de test alpin 581 tuiles, 9 répertoires feuilles, 26 lectures de données (5,7 Mo lus), 58 s au premier passage (1 requête/s) ; Jeu de test jurassien 537 tuiles, 8 feuilles, 26 lectures, 35 s. Les tuiles sont identiques octet pour octet à celles de `pmtiles extract` (go-pmtiles 1.31.2), qui n'est plus nécessaire (test `protomaps_extract`, ignoré par défaut). L'extrait est gardé dans `data/cache/protomaps/extracts/<site>-<build>-z<zoom>-<emprise>.pmtiles` et réutilisé.
- Build en usage (X26, #160) : deux extraits du même build sont identiques à l'octet près, mais chaque build quotidien changeait toute l'archive, donc toute l'archive partait dans chaque mise à jour (3 587 664 octets sur 3 621 860 pour la mise à jour d'un site du 16 au 17/11/2026). Cause mesurée sur les builds 20261112 à 20261116 pour trois sites (Alpes, Jura, Pyrénées) : les métadonnées portent l'heure de réplication OSM (`planetiler:osm:osmosisreplicationtime`), et 5 à 9 tuiles des zooms 2 à 9, chacune couvrant des centaines de kilomètres, changent chaque jour avec les modifications faites ailleurs (0,6 à 1,9 Mo par jour) ; dans l'emprise (tuiles du zoom 15), 0 à 9 tuiles sur 399, aucune sur 360, et 0, 1 ou 16 sur 120 changent d'un build au suivant. Le build en usage, noté dans `extracts/<site>-z<zoom>-<emprise>.current.json`, est donc gardé tant que moins de 5 % des tuiles du zoom maximal de l'emprise diffèrent de lui (comparaison après décompression) et qu'il a moins de 7 jours de plus que le dernier build ; sinon (ou au changement de schéma, ou si son extrait a quitté le cache) le dernier build est adopté. Les petites modifications s'additionnent contre le build en usage. La date affichée (`data_date`, sources) est celle du build en usage, et une note du manifeste dit le build comparé et le nombre de tuiles changées. Mesure (builds 20261112 à 20261116, extraits gardés depuis le 12) : mise à jour du fond 0 octet chaque jour pour les trois sites, sauf le 16 pour le site pyrénéen (16 tuiles sur 120 : archive de 3 106 870 octets adoptée). Pack complet, build 20261115 puis 20261116 : 251 710 et 316 330 octets à télécharger (manifeste et `pack.meta.json`) au lieu de 3,8 et 4,9 Mo. Test `daily_builds_keep_the_archive_until_the_box_changes`.
- Comparaison sans extrait (#164) : pour comparer, seules les tuiles du zoom maximal de l'emprise sont lues dans le nouveau build (en-tête, répertoires feuilles de ces tuiles, données en plages fusionnées : `extract::Archive::tiles_at`) ; l'extrait de toute l'emprise n'est lu et écrit que si le build est adopté, sans relire les tuiles déjà lues. Avant, l'extrait complet de chaque nouveau build était téléchargé et gardé pour chaque site (2 518 requêtes, 545 Mo par jour en production pour 268 sites). Mesure (exemple `osm_daily_bytes`, copie du cache en liens physiques, build en usage 20261115, huit sites de huit massifs : val-cenis, la-morte, freissinieres, gerardmer, les-estables, cauterets-pont-d-espagne, bolandoz, le-hohwald ; mêmes décisions et mêmes tuiles changées avant et après, tous gardés) : build 20261116, 134 requêtes et 26 043 759 octets avant, 24 requêtes et 5 886 750 octets après ; build 20261117, 127 requêtes et 25 200 734 octets avant, 22 requêtes et 5 869 148 octets après (−77 % ; il reste surtout les répertoires feuilles du zoom 15). Test : un jour gardé, seules les données des tuiles du zoom 15 sont lues ; un jour adopté, chaque octet de données une fois.
- Style : `assets/protomaps/style-light-fr.json`, généré une fois par `assets/protomaps/generate.mjs` avec `@protomaps/basemaps` 5.7.2 (thème « light », libellés en français) ; polices Noto Sans (Regular, Medium, Italic) et sprite `v4/light` téléchargés depuis `protomaps.github.io/basemaps-assets` (source `protomaps-assets`). La pile Devanagari du style n'est pas copiée.
- Licence ODbL (Produced Work), attribution « Protomaps © OpenStreetMap ».

## Mesures (14-15/11/2026, build Protomaps 20261114)

Taille de l'archive selon le zoom maximal (`examples/basemap_sizes.rs` : mêmes tuiles réécrites en PMTiles pour chaque zoom maximal ; le zoom 17 n'a été téléchargé que pour le jeu de test alpin) :

| Zoom max | Plan IGN, test alpin | Plan IGN, test jurassien | OSM, test alpin | OSM, test jurassien |
|---|---|---|---|---|
| 12 | 2,50 Mo (36 tuiles) | 2,13 Mo (31) | 2,24 Mo | 2,81 Mo |
| 13 | 4,08 Mo (72) | 3,78 Mo (67) | 2,57 Mo | 3,23 Mo |
| 14 | 5,83 Mo (182) | 5,76 Mo (177) | 2,96 Mo | 3,76 Mo |
| 15 | 8,13 Mo (581) | 8,07 Mo (537) | 3,59 Mo | 4,55 Mo |
| **16** | **11,34 Mo (2 061)** | **11,16 Mo (1 829)** | — | — |
| 17 | 16,69 Mo (7 833) | non mesuré | — | — |

Les zooms 0 à 12 pèsent déjà 2 à 2,5 Mo : peu de tuiles, mais chacune couvre une large région.

**Choix : Plan IGN z0-16, OSM z0-15.**

- Plan IGN : à z16 la précision des coordonnées est de 0,1 m (tuile de 430 m à 45° N, 4 096 unités) ; au-delà, MapLibre agrandit le z16. Capture comparée au zoom 17,5 sur le col d'Izoard (z16 agrandi contre vraies tuiles z17) : routes, ruisseaux et chemins identiques, seuls les libellés changent de place. Le z17 coûterait +5,4 Mo (+47 %) et 5 772 requêtes de plus par site (≈ 25 min au débit observé de 3 à 4 tuiles/s) pour des couches qui n'apparaissent qu'aux zooms 17-18 (bornes, parcelles, adresses).
- OSM : 3,6 et 4,6 Mo pour tous les zooms du build ; aucune raison de couper.
- Avec style, sprite et glyphes : Plan IGN 13,1 Mo (jeu de test alpin) et 13,0 Mo (jeu de test jurassien), OSM 4,6 et 5,6 Mo.

L'extrait de `pmtiles extract` faisait 3,57 et 4,53 Mo (581 et 537 tuiles), l'extrait Rust 3,59 et 4,55 Mo (mêmes tuiles, répertoires écrits par la crate `pmtiles`).

Temps : à froid, le téléchargement des tuiles IGN domine (≈ 2 000 tuiles par site, 3 à 4 tuiles/s observées avec une requête à la fois, soit 8 à 10 min) ; l'extrait Protomaps prend 13 s ; depuis le cache, chaque fond s'écrit en 0,2 à 0,3 s.

## Contrôles

- `tests/real_packs.rs` (`basemaps_are_complete`) : archives MVT lisibles par le lecteur, tuile z14 présente au centre de la commune, style pointant vers l'archive du pack, chaque glyphe requis présent.
- Visualiseur (Chrome sans tête) : les deux fonds s'affichent ; requêtes vers `127.0.0.1` seulement, hors les deux scripts du CDN.
- `pmtiles verify` (go-pmtiles 1.31.2) : les 14 archives des deux packs passent (15/11/2026 : fonds OSM et Plan IGN, pentes, neige, exposition aux avalanches). Avant la copie corrigée de la crate `pmtiles` (`vendor/pmtiles`), 10 sur 12 étaient refusées (« Leaf directories offset=0 must not be 0 »).
