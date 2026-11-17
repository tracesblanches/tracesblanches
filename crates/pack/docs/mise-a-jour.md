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
- `deltas` : un paquet vers la dernière version depuis la base (sauf si la base est la dernière) et depuis chacune des 8 dernières versions ;
- `changes` : chaque fond de carte, couche et voix de la dernière version avec son changement depuis la précédente (`added`, `changed`, `unchanged`, `removed`) et sa validité (`generated_at`, `valid_from`, `valid_until`, `validity`).

La base n'est reconstruite que quand le paquet depuis la base dépasse un dixième du pack (relief, exposition aux avalanches, fonds de carte). Les couches quotidiennes (neige, BERA) restent des petits paquets.

## Paquet (`.skb`)

```text
"SKMPBND2"                  8 octets
longueur de l'en-tête       u64 petit-boutiste
en-tête                     l'Update en JSON compact, compressé gzip
manifeste                   gzip (longueur : manifest_gzip_bytes), sauf si à jour
blobs de fetch[]            dans l'ordre : gzip si fetch[].gzip_bytes est présent (sa longueur),
                            sinon tels quels (longueur fetch[].bytes)
```

`Update` (`update::Update`, format `skimap-pack-update` version 2) : `site`, `from_version` (nul : depuis rien), `to_version`, `manifest` (empreinte et taille du `manifest.json` décompressé), `fetch` (contenus absents de la version de départ, chacun une fois, avec les chemins qui les utilisent ; `gzip_bytes` quand le blob est stocké compressé), `fetch_bytes` (contenus décompressés), `reused_files`, `removed`, `files`, `total_bytes`, `valid_for`, `changes`, `manifest_gzip_bytes`.

Un paquet est servi en binaire : ni Cloudflare ni le serveur ne le compressent en transit. Le paquet compresse donc lui-même (gzip niveau maximal, sans nom ni date : même entrée, mêmes octets) l'en-tête, le manifeste et chaque blob texte (`update::compressible` : chemins en `.json`, `.geojson`, `.tsv`, `.txt`, `.csv` et les glyphes `.pbf`) quand il en sort plus petit ; les gros fichiers déjà compressés partent tels quels (PMTiles, SKR gzip, PNG, Opus). Le téléphone décompresse chaque blob en l'installant et vérifie la taille et le SHA-256 du contenu décompressé (jamais plus d'octets que la taille annoncée) ; le pack installé est identique à celui du serveur.

Compatibilité : un paquet de format 1 (magie `SKMPBND1`, en-tête JSON en clair, aucun blob compressé) s'installe toujours. Une app antérieure au format 2 refuse proprement un paquet `SKMPBND2` (« not a skimap bundle ») et garde son pack : il faut mettre l'app à jour.

## Synchronisation (ce que fait l'app)

1. `GET /v1/sites/<site>/index.json` avec `If-None-Match` = `index_etag` de l'état (`sync::read_state`). Réponse 304 : fini, **une requête, corps vide**.
2. Sinon `sync::plan(&index, état)` : rien si à jour ; le paquet depuis la version installée s'il est publié ; sinon le paquet depuis la base si l'état a la même base (téléphone resté hors ligne des semaines : petit paquet, pas le pack entier) ; sinon la base puis le paquet depuis la base.
3. Pour chaque paquet : télécharger le fichier (reprise par `Range` / `If-Range` sur l'`ETag`, qui est l'empreinte), puis `update::apply_bundle(pack_dir, fichier)`. Si l'erreur commence par `update::MISSING_CONTENTS` (contenus que le téléphone n'a plus), recommencer avec `sync::plan_full`.
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
