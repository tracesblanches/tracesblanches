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
"SKMPBND1"                  8 octets
longueur de l'en-tête       u64 petit-boutiste
en-tête                     l'Update en JSON compact
manifeste                   gzip (longueur : manifest_gzip_bytes), sauf si à jour
blobs de fetch[]            tels quels, dans l'ordre, longueurs de fetch[].bytes
```

`Update` (`update::Update`, format `skimap-pack-update` version 1) : `site`, `from_version` (nul : depuis rien), `to_version`, `manifest` (empreinte et taille du `manifest.json` décompressé), `fetch` (contenus absents de la version de départ, chacun une fois, avec les chemins qui les utilisent), `fetch_bytes`, `reused_files`, `removed`, `files`, `total_bytes`, `valid_for`, `changes`, `manifest_gzip_bytes`.

Seul le manifeste est compressé (≈ 60 ko au lieu de 280 ko sur le jeu de test alpin) : les gros fichiers le sont déjà (PMTiles, SKR gzip, Opus). Le téléphone n'a rien à décompresser d'autre et lit chaque blob directement dans le paquet, sans le déballer d'abord.

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
