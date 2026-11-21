# Téléchargement et mise à jour des packs (K2 #52, VO6 #129, H2 #133, E1 #125)

[← skimap pour Android](../README.md)

Formats et API : [`crates/pack/docs/mise-a-jour.md`](../../crates/pack/docs/mise-a-jour.md), [`crates/server/docs/api.md`](../../crates/server/docs/api.md).

## Module réseau `:sync`

- Seul code qui ouvre une connexion (`HttpSource`) et seul module à déclarer `INTERNET` (raison dans son manifeste). L'app ne l'avait plus : la permission revient pour le téléchargement seulement ; MapLibre reste hors ligne (`MapLibre.setConnected(false)`, styles en `file://`).
- Documents revalidés (`If-None-Match` → 304), paquets repris après coupure (`Range: bytes=<reçu>-`, `If-Range: "<sha256>"`), réponse 200 ou 416 à une reprise → depuis zéro, longueur reçue vérifiée (une connexion coupée n'est pas toujours une erreur pour la pile HTTP), `User-Agent` (contrôle de navigateur de Cloudflare), `Accept-Encoding: identity` pour les paquets.
- Serveur : `https://skimap.warnier.net` ; la préférence `server_url` (fichier `skimap`) le change pour les tests. HTTP en clair permis vers `10.0.2.2`, `127.0.0.1` et `localhost` dans la version de débogage seulement (`src/debug`).

## Une synchro (`PackSyncer`)

1. Index du site avec l'`ETag` noté : 304 → fini, une requête, corps vide.
2. Plan du noyau (`sync_plan`) : rien, le delta depuis la version installée, ou la base puis le delta. Un pack copié à la main sans état est reconnu par sa version. Sur réseau mesuré, seuls les octets encore à télécharger comptent pour la limite de 10 Mo : un paquet déjà entier dans `.<site>.download` (SHA-256 vérifié) ne compte pour rien, un paquet partiel pour ce qui lui manque (#176 : une base téléchargée en Wi-Fi puis interrompue avant l'installation s'installe ensuite sur n'importe quel réseau).
3. Chaque paquet : téléchargé dans `.<site>.download/<sha256>.part` (repris après coupure), contrôlé entier (`verify_bundle` : longueur et SHA-256, son nom ; faux → retéléchargé une fois depuis zéro), installé par le noyau (`install_bundle` : chaque blob revérifié avant toute écriture, nouvelle version construite à côté puis échangée).
4. Fichier reconstruit d'un correctif de tuiles différent de celui du serveur (`PatchFailed`, #160 : le paquet de format 3 envoie les PMTiles changés tuile par tuile) → le même plan, fichiers entiers (`sync_plan_whole_files`) ; contenus manquants sur le téléphone (`MissingContents`) → plan complet (`sync_plan_full`).
5. État noté (`record_sync`) ; dossier de téléchargement supprimé. Le pack en place reste complet et lisible à chaque étape ; serveur injoignable → exception, rien ne change.
6. Une synchro d'un pack à la fois dans le processus (verrou par dossier) : la mise à jour quotidienne et le téléchargement du même pack partagent le dossier de téléchargement.

## WorkManager (`PackSync`, `PackSyncWorker`)

- Téléchargement demandé : réseau requis ; **réseau non mesuré au-delà de 10 Mo**, sauf « Utiliser les données mobiles » ; stockage non bas ; reprise exponentielle (30 s) après une coupure.
- Mise à jour quotidienne des packs installés (et de leur voix) : réseau et batterie non faible, désactivable dans le menu ; sur réseau mesuré, un plan de plus de 10 Mo n'est pas commencé et attend le Wi-Fi. Rien d'autre ne tourne en arrière-plan (E1) ; ni service de premier plan ni notification : un téléchargement interrompu par la limite de 10 min de WorkManager reprend où il en était.
- Mise à jour quotidienne (`DailyUpdate`, #176) : chaque pack est essayé ; un pack en échec (index ou paquet injoignable, paquet qui ne s'installe pas) garde sa version et sa date « Mis à jour le… », et laisse les suivants se mettre à jour ; coupure réseau ou serveur → tout est repris plus tard (les packs déjà à jour répondent 304). Un plan laissé pour le Wi-Fi ne remplace pas un téléchargement déjà demandé (`KEEP`).
- Journal des synchros : une ligne datée par pack et par synchro (mise à jour, à jour, en attente du Wi-Fi avec la taille, échec avec sa raison), dans logcat et dans `sync-journal.txt` (stockage externe de l'app, à côté de `packs/`, 400 dernières lignes) : ce qui s'est passé la nuit y reste.
- Mode avion : les travaux attendent leur réseau, la carte et les fiches marchent (tout vient des packs).
- Une installation terminée rouvre la carte avec les nouveaux packs (suivi des travaux par Flow, seulement quand l'écran vit).

## Carte de France des domaines

- 219 domaines du recensement : carrés installés, disques publiés par le serveur, points non publiés ; toucher la carte liste les domaines les plus proches du toucher, recherche par nom ou commune, domaines les plus proches de la position (une position grossière, au toucher).
- Par domaine : taille du premier téléchargement, « En attente du Wi-Fi » ou « de réseau », progression, « Mettre à jour » quand une version plus récente est publiée, « Supprimer ».
- Catalogue du serveur gardé sur le téléphone (`files/server/catalog.json`) : hors ligne, la dernière liste, signalée.
- Packs par domaine : le serveur ne publie pas de pack par zone (cadre, massif). Le mode libre les montre ensemble.

## Voix par langue (VO6)

- Un paquet de voix est un pack à part (`files/voice/<site>/<langue>/voice/…`), synchronisé exactement comme un pack de site (index, paquets, état).
- Téléchargé avec le pack, pour la langue de l'app (français sur un téléphone en français, sinon anglais), quand le catalogue du serveur le liste : champ `voice` d'un site, `[{lang, index_url, latest, first_download_bytes}]` (à publier par le serveur ; testé sur un catalogue de test).
- Une nouvelle langue remplace l'ancienne seulement une fois installée : hors ligne, l'ancienne voix reste.

## Données à venir (schémas)

- Couche de pack `stations` (points) : `name`, `emergency` `[{label, phone}]` (112 toujours en premier), `fee_outlets` `[{name, address, phone}]`, `prices` `[{label, price_eur, date}]`, `phones`, `websites` `[{label, url}]`, `source`, `updated` ; fiche prête (`crates/ffi/src/card.rs`).
- Zones de redevance (H2) : couche `fee-zones` (polygones) `{fee_id, name, issuer, kind, prices: [{label, price_eur, date}]}`, `kind` : `site`, `groupement`, `departement`, `region`, `national` ; `fee_id` identique d'un pack à l'autre pour un même titre (ENJ, Nordic Pass). Lue par le noyau (`PackSet.feeZones`) ; donnée de test dans le pack de test ; affichage dès qu'un style validé la dessine.
