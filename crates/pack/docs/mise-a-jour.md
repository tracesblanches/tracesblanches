# Mises à jour et synchronisation (K2, S0, E1)

[← skimap-pack](../README.md)

Tout est côté téléphone, sans réseau dans la crate : l'app (Kotlin, K2) fait les requêtes HTTP, la crate décide quoi télécharger (`sync::plan`) et installe (`update::apply_bundle`). Le serveur ne calcule rien par requête : ce qui suit est produit une fois par domaine à chaque publication (`skimap-server publish`, crate `server`).

## Ce que publie le serveur

| URL | Cache | Contenu |
|---|---|---|
| `/v1/catalog.json` | 60 s, `ETag` | tous les sites : `site` (identifiant, nom, emprise), `index_url`, `latest`, `first_download_bytes` |
| `/v1/sites/<site>/index.json` | 60 s, `ETag` | `SiteIndex` (ci-dessous) |
| `/v1/bundles/<sha256>.skb` | 1 an, `immutable`, `Range` | un paquet : le nom est l'empreinte du fichier, une nouvelle version est un nouveau nom |

`SiteIndex` (`sync::SiteIndex`, format `skimap-site-index` version 1) :

- `latest` : version (`pack_version`), date de publication, empreinte du manifeste, taille, nombre de fichiers, validité (`valid_for`), `personal_sources` ;
- `base_version` et `base` : le pack entier à la version de base (`BundleRef` : `from` nul, `to`, `sha256`, `bytes`, `url`, `blobs`) ;
- `deltas` : un paquet vers la dernière version depuis la base (sauf si la base est la dernière) et depuis chacune des 8 dernières versions, chaque fichier entier (format 2 : ce que lit une app de format 2) ;
- `patched_deltas` (#160, absent quand vide) : les deltas de `deltas` qui sont plus petits avec des PMTiles envoyés tuile par tuile (format 3, [ci-dessous](#correctifs-de-tuiles-format-3-160)), mêmes `from` et `to`. Une app de format 2 ignore ce champ inconnu, l'index reste en version 1 ; une app de format 3 le préfère ;
- `changes` : chaque fond de carte, couche et voix de la dernière version avec son changement depuis la précédente (`added`, `changed`, `unchanged`, `removed`) et sa validité (`generated_at`, `valid_from`, `valid_until`, `validity`).

La base n'est reconstruite que quand le paquet depuis la base dépasse un dixième du pack (relief, exposition aux avalanches, fonds de carte). Les couches quotidiennes (neige, BERA) restent des petits paquets.

## Paquet (`.skb`)

```text
"SKMPBND2" ou "SKMPBND3"    8 octets (SKMPBND3 : le paquet contient un correctif, format 3)
longueur de l'en-tête       u64 petit-boutiste
en-tête                     l'Update en JSON compact, compressé gzip
manifeste                   gzip (longueur : manifest_gzip_bytes), sauf si à jour
blobs de fetch[]            dans l'ordre : correctif si fetch[].patch est présent (longueur
                            patch.bytes), gzip si fetch[].gzip_bytes est présent (sa longueur),
                            sinon tels quels (longueur fetch[].bytes)
```

`Update` (`update::Update`, format `skimap-pack-update` version 2, ou 3 quand un blob est un correctif) : `site`, `from_version` (nul : depuis rien), `to_version`, `manifest` (empreinte et taille du `manifest.json` décompressé), `fetch` (contenus absents de la version de départ, chacun une fois, avec les chemins qui les utilisent ; `gzip_bytes` quand le blob est stocké compressé, `patch` quand il est un correctif : `from_sha256`, `bytes`, `tiles_added`, `tiles_changed`, `tiles_removed`), `fetch_bytes` (contenus décompressés), `reused_files`, `removed`, `files`, `total_bytes`, `valid_for`, `changes`, `manifest_gzip_bytes`.

Un paquet est servi en binaire : ni Cloudflare ni le serveur ne le compressent en transit. Le paquet compresse donc lui-même (gzip niveau maximal, sans nom ni date : même entrée, mêmes octets) l'en-tête, le manifeste et chaque blob texte (`update::compressible` : chemins en `.json`, `.geojson`, `.tsv`, `.txt`, `.csv` et les glyphes `.pbf`) quand il en sort plus petit ; les gros fichiers déjà compressés partent tels quels (PMTiles, SKR gzip, PNG, Opus). Le téléphone décompresse chaque blob en l'installant et vérifie la taille et le SHA-256 du contenu décompressé (jamais plus d'octets que la taille annoncée) ; le pack installé est identique à celui du serveur.

Compatibilité : un paquet de format 1 (magie `SKMPBND1`, en-tête JSON en clair, aucun blob compressé) s'installe toujours. Une app antérieure au format 2 refuse proprement un paquet `SKMPBND2` (« not a skimap bundle ») et garde son pack : il faut mettre l'app à jour. Un paquet est écrit à la plus petite version qui le lit : `SKMPBND2` et format 2 sans correctif (à l'octet près le paquet d'avant), `SKMPBND3` et format 3 seulement avec un correctif. Une app de format 2 ne voit jamais ces derniers (ils ne sont que dans `patched_deltas`) ; si elle en recevait un, elle le refuserait (« not a skimap bundle ») et garderait son pack.

## Correctifs de tuiles (format 3, #160)

Un nouveau build du fond OSM, la neige du jour ou toute couche PMTiles change quelques tuiles d'une archive et laisse les autres identiques à l'octet. Pour chaque fichier `.pmtiles` du paquet qui existe au même chemin dans la version de départ, le serveur (`update::tile_patches`) découpe la nouvelle archive selon sa propre structure (en-tête, répertoire racine, métadonnées, chaque répertoire feuille, chaque contenu de tuile distinct, et les octets entre eux) et décrit chaque région soit comme une copie de l'ancienne archive (mêmes octets : tuile inchangée, métadonnées identiques, bourrage de zéros), soit comme des octets envoyés (tuile ajoutée ou changée, répertoire dont les décalages ont bougé). Il applique le correctif à l'ancien fichier, vérifie qu'il redonne le nouveau à l'octet près, et ne le garde que s'il est plus court que le fichier ; un fichier qui n'est pas une archive PMTiles v3 lisible part entier. L'en-tête du paquet compte les tuiles ajoutées, changées et supprimées (par identifiant de tuile).

Le téléphone ne réécrit pas l'archive à partir des identifiants de tuiles : il lui faudrait écrire les répertoires exactement comme l'outil du serveur (ordre, regroupement, taille des feuilles, gzip de chaque répertoire). Il copie des plages de l'ancien fichier et ajoute les octets reçus (`patch::apply`), quel que soit l'outil qui a écrit l'une ou l'autre archive.

```text
"SKMPTCH1"                  8 octets
n                           nombre d'opérations (varint LEB128)
n fois : (long << 1) | type 0 : copie, 1 : octets envoyés
         décalage           copie seulement : position dans l'ancien fichier
octets envoyés              dans l'ordre des opérations de type 1
```

Les opérations écrivent le nouveau fichier de son premier à son dernier octet. Avant d'écrire, `apply_bundle` vérifie que la version installée a le fichier visé (`from_sha256`), chaque copie dans l'ancien fichier, et la longueur totale ; à la fin, la longueur et le SHA-256 du fichier reconstruit contre le manifeste. Tout écart (fichier visé absent, ancien fichier altéré, correctif tronqué ou trop long) donne une erreur commençant par `update::PATCH_FAILED` (« patch failed ») : rien n'est installé, le pack reste tel quel, et l'app reprend la même mise à jour avec les fichiers entiers (`sync::plan_whole_files`).

## Synchronisation (ce que fait l'app)

1. `GET /v1/sites/<site>/index.json` avec `If-None-Match` = `index_etag` de l'état (`sync::read_state`). Réponse 304 : fini, **une requête, corps vide**.
2. Sinon `sync::plan(&index, état)` : rien si à jour ; le paquet depuis la version installée s'il est publié ; sinon le paquet depuis la base si l'état a la même base (téléphone resté hors ligne des semaines : petit paquet, pas le pack entier) ; sinon la base puis le paquet depuis la base.
3. Pour chaque paquet : télécharger le fichier (reprise par `Range` / `If-Range` sur l'`ETag`, qui est l'empreinte), puis `update::apply_bundle(pack_dir, fichier)`. Si l'erreur contient `update::PATCH_FAILED` (fichier reconstruit d'un correctif différent de celui du serveur), recommencer avec `sync::plan_whole_files` (les mêmes deltas, fichiers entiers) ; si elle commence par `update::MISSING_CONTENTS` (contenus que le téléphone n'a plus), recommencer avec `sync::plan_full` (la base, puis le delta depuis la base aux fichiers entiers).
4. `sync::write_state` : version, base, `ETag` de l'index.

`apply_bundle` vérifie la longueur du fichier (téléchargement incomplet refusé), la taille et le SHA-256 de chaque blob, et que chaque contenu de la nouvelle version est soit dans le pack installé, soit dans le paquet, **avant d'écrire quoi que ce soit**. La nouvelle version est construite dans `.<pack>.new` (fichiers inchangés en liens physiques, copiés si le système de fichiers refuse), puis échangée avec l'ancienne (`.<pack>.old`) ; `update::recover` termine ou annule un échange interrompu. L'état est dans `.<pack>.sync.json`, à côté du pack.

Hors ligne, rien ne change : le pack installé reste complet et lisible, chaque couche porte sa date de validité ; aucune fonction du téléphone n'attend le serveur.

## Mesures (15/11/2026, sorties réelles, sans sources à usage personnel)

Publication de deux jours de neige consécutifs (simulation du 13/11 puis du 14/11), synchronisation par `crates/server/examples/phone_sync.rs` (mêmes appels que l'app) :

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Premier téléchargement | 2 requêtes, 43,50 Mo (index 13,8 ko + base 43,49 Mo) | 2 requêtes, 39,98 Mo |
| Synchro sans changement | 1 requête, 304, 0 octet de corps | idem |
| Synchro quotidienne (neige J-1) | 2 requêtes, 344 ko (paquet 330 ko : 6 fichiers, 962 repris) | 2 requêtes, 137 ko (paquet 123 ko) |
| Pack installé identique à celui du pipeline | oui (969 fichiers, SHA-256) | oui (1 208 fichiers) |

L'index est servi en JSON (14 ko) ; Cloudflare le compresse en transit. Tests : `tests/update.rs` (premier téléchargement, delta, réutilisation par contenu, doublons, blob corrompu, échange interrompu, paquet tronqué, contenus manquants), `src/sync.rs` (plans), `crates/server/tests/packs.rs` (de bout en bout en HTTP).

## Mesures du format 2 (17/11/2026, sorties réelles, sans sources à usage personnel, #160)

Huit sites de huit massifs (Haute-Maurienne, Briançonnais, Aravis, Vercors, Pyrénées, Cantal, Vosges, Jura), copie de travail des sorties du 15/11, trois runs AROME successifs rejoués (`SKIMAP_AROME_RUN` : 16/11 09 h UTC, 17/11 03 h, 17/11 09 h), `build-all --stale` puis `skimap-server publish` après chaque run, même procédure avant (main 541306e) et après :

| Site | Par run (03 h → 09 h) avant | après | Quotidien (J-1 09 h → J 09 h) avant | après | Premier téléchargement avant | après |
|---|---|---|---|---|---|---|
| bessans | 1 425 566 | 79 705 | 1 898 033 | 489 776 | 27,77 Mo | 24,71 Mo |
| domaine-nordique-de-nevache | 1 436 249 | 82 671 | 2 224 892 | 747 245 | 41,20 Mo | 37,77 Mo |
| le-grand-bornand | 1 392 206 | 74 007 | 1 603 741 | 224 797 | 21,54 Mo | 18,99 Mo |
| autrans | 1 053 786 | 82 760 | 1 262 209 | 152 148 | 34,54 Mo | 30,58 Mo |
| font-romeu-odeillo-via | 1 334 878 | 65 865 | 1 435 646 | 96 236 | 16,59 Mo | 14,26 Mo |
| le-lioran | 570 000 | 55 326 | 676 567 | 100 835 | 12,81 Mo | 10,77 Mo |
| gerardmer | 1 448 029 | 82 442 | 1 651 602 | 131 238 | 43,79 Mo | 39,29 Mo |
| les-rousses | 1 463 694 | 79 012 | 1 683 088 | 130 015 | 24,69 Mo | 20,37 Mo |
| **8 packs** | **10,12 Mo** | **0,60 Mo** | **12,44 Mo** | **2,07 Mo** | **222,9 Mo** | **196,7 Mo** |

Octets du paquet (`.skb`) servi au téléphone. Un paquet par run (Briançonnais, 82,7 ko) : en-tête 2,6 ko, manifeste 25,0 ko, briefing de l'assistant 14,8 ko (275 ko décompressé), boucles suggérées 11,8 ko, qualité de neige 11,0 ko, `pack.meta.json` 6,6 ko, météo heure par heure 5,0 ko (90 ko), métadonnées 5,4 ko. Le paquet quotidien y ajoute la neige du jour (PMTiles déjà compressés : 590 ko sur ce site, 365 ko à Bessans), seule part au-dessus de 200 ko. Synchronisation de bout en bout en HTTP (`examples/phone_sync`, serveur local) : base puis paquet quotidien, pack installé identique à l'octet à celui du pipeline sur les huit sites, puis 304.

## Mesures du format 3 (18/11/2026, sorties réelles, sans sources à usage personnel, #160)

Dix sites de neuf massifs dont trois petites emprises (le Somport, Laguiole, La Chapelle : 100 à 182 tuiles en tout), copie de travail des sorties du 15/11 (cache en liens physiques), deux versions successives de chaque pack par `skimap build-pack` : build Protomaps 20261117 et neige du 16/11, puis build 20261118 adopté et neige du 17/11 (le jour qui domine : adoption d'un build), publiées par `skimap-server publish`, synchronisées en HTTP par `examples/phone_sync` sur un serveur local, une fois en format 3 et une fois aux fichiers entiers (`PHONE_SYNC_WHOLE_FILES=1`, ce que télécharge une app de format 2) :

| Site (massif) | Paquet format 2 | Paquet format 3 | dont `osm.pmtiles` (fichier → correctif) | Tuiles du fond changées |
|---|---|---|---|---|
| le-somport (Pyrénées) | 1 670 231 | 587 422 | 1 629 631 → 565 487 | 5, zooms 4 à 8 |
| laguiole (Aubrac) | 2 171 146 | 729 746 | 2 151 814 → 710 335 | 5, zooms 3 à 7 |
| la-chapelle (Ardennes) | 2 112 922 | 927 119 | 2 093 914 → 908 034 | 6, zooms 3 à 8 |
| le-lioran (Cantal) | 2 989 550 | 1 020 610 | 2 951 763 → 1 000 252 | 6, zooms 3 à 7 |
| le-grand-bornand (Aravis) | 3 084 075 | 1 403 449 | 2 941 099 → 1 312 600 | 7, zooms 3 à 9 |
| bessans (Haute-Maurienne) | 3 584 977 | 1 657 846 | 3 181 095 → 1 304 285 | 9, zooms 3 à 8, 14, 15 |
| les-rousses (Jura) | 4 274 409 | 1 384 726 | 4 254 119 → 1 364 357 | 7, zooms 3 à 9 |
| gerardmer (Vosges) | 7 449 867 | 2 321 700 | 7 429 951 → 2 301 706 | 10, zooms 3 à 9 |
| autrans (Vercors) | 5 974 666 | 2 374 449 | 5 934 413 → 2 353 318 | 11, zooms 3 à 9 |
| font-romeu-odeillo-via (Pyrénées-Orientales) | 3 126 632 | 691 781 | 3 106 797 → 671 867 | 6, zooms 3 à 7, 15 |
| **10 packs** | **36,44 Mo** | **13,10 Mo** | | |

Octets du paquet (`.skb`) servi au téléphone. Le correctif du fond est fait presque tout entier des tuiles larges des zooms 3 à 9 (85 à 650 ko chacune), qui changent à chaque build avec des modifications faites à des centaines de kilomètres ; dans l'emprise, 0 ou 1 tuile du zoom 15 a changé ; répertoires et en-tête : 1,7 à 2,4 ko. La neige du jour, dont presque toutes les tuiles changent, gagne peu (Bessans : 192 440 → 175 475 octets pour la hauteur de neige ; le Somport, 2 tuiles sur 4 changées : 19 298 → 503). Un jour où seule la neige change (Bessans, neige du 17 puis du 18/11) : 70 086 → 69 236 octets.

Reconstruction du fond sur le téléphone (`examples/pmtiles_patch` : ancien fichier sur disque, correctif lu en flux, SHA-256 du résultat) : 1 à 4 ms sur l'hôte, 6 à 25 ms sur l'émulateur Android 17 x86_64 (Pixel 8) ; installation du paquet entier par `apply_bundle` : 0,03 à 0,04 s sur l'hôte, comme le paquet aux fichiers entiers. Fichier reconstruit identique à l'octet sur les dix sites (hôte et émulateur), pack installé identique à celui du pipeline (`diff -r`, 60 à 65 fichiers) dans les deux formats, puis 304. Repli vérifié de bout en bout : ancien fichier de neige altéré d'un octet (même taille) sur le téléphone → « patch failed », pack intact, paquet aux fichiers entiers installé, pack identique. Publication des dix sites avec les deux jeux de deltas : 1,4 s. Les paquets de format 2 (base et deltas) sont identiques à l'octet à ceux du serveur de main (c5ee6d9) publiant les mêmes packs ; le lecteur de main installe le delta de format 2 depuis le nouvel index (pack identique) et refuse un paquet de format 3 (« not a skimap bundle »), pack inchangé. Builds 20261114 → 20261115 sur 38 sites : correctif du fond de 0,41 à 3,40 Mo (16 à 63 % de l'archive), reconstruit identique partout.

Tests : `tests/tile_patches.rs` (reconstruction à l'octet, archives identiques, refus : ancien fichier différent, longueur, correctif tronqué ou trop long, copie hors du fichier, pas une archive ; paquet de format 3 contre format 2 à l'octet près inchangé, ancien fichier altéré ou absent → « patch failed » sans rien écrire puis fichiers entiers), `src/patch.rs`, `src/sync.rs` (`patched_deltas_first`), `crates/server/tests/packs.rs` (`patched_deltas_send_the_changed_tiles`, en HTTP).
