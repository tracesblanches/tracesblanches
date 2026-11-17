# Coûts estimés (S0, E1) au 15/11/2026

[← skimap-server](../README.md)

Contrainte produit : gratuit ou paiement unique, donc aucun coût récurrent par utilisateur. Le serveur ne calcule rien par requête et Cloudflare sert les fichiers : l'origine (serveur) voit une charge qui dépend du nombre de domaines, pas du nombre de téléphones.

## Mesures de base (sorties réelles, sans sources à usage personnel)

| | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Paquet de base (premier téléchargement) | 43,49 Mo | 39,97 Mo |
| Paquet quotidien (neige J-1) | 330 ko | 123 ko |
| Index (`index.json`), compressé en transit par Cloudflare | 13,8 ko (304 sans changement) | 13,8 ko |
| Synchro quotidienne | 2 requêtes, 344 ko | 2 requêtes, 137 ko |
| Synchro sans changement | 1 requête, 304 | 1 requête, 304 |
| Publication quotidienne sur le poste (neige 0,5 s + rasters du routeur 1,1 s + pack 1,5 s + publication 0,2 s) | 3,3 s | 2,5 s |
| `build-all` complet (le cas lourd) | 149 s, 1 615 s de CPU, pic 2,73 Gio | |
| Serveur : mémoire au repos / après 20 téléchargements parallèles | 7,3 Mo / 10,7 Mo (natif) ; 29,7 Mio sous QEMU (émulateur compris) | |

## Hypothèses par utilisateur

1,5 zone par utilisateur ; saison de 4 mois ; une synchro par zone et par jour (le téléphone n'a jamais besoin de se connecter souvent : un téléphone resté hors ligne des semaines reçoit le paquet depuis la base, de taille comparable) ; le pack entier une fois par saison ; une mise à jour des fonds de carte par mois (paquet de l'ordre de 10 Mo).

Par utilisateur et par mois de saison : 26 Mo et 90 requêtes ; le mois des premiers téléchargements : 89 Mo.

| Utilisateurs | Trafic, mois ordinaire | Trafic, mois de pointe | Requêtes par mois | Servi par |
|---|---|---|---|---|
| 1 000 | 26 Go | 89 Go | 90 000 | cache Cloudflare |
| 10 000 | 260 Go | 0,9 To | 0,9 M | cache Cloudflare |
| 100 000 | 2,6 To | 8,9 To | 9 M | cache Cloudflare (ou R2, ci-dessous) |

Origine (serveur) : chaque nouveau paquet est lu une fois par point de présence Cloudflare qui le demande (et une seule fois avec le cache hiérarchisé, gratuit), l'index au plus une fois par minute et par point de présence. Pour 219 domaines et une poignée de points de présence français : de l'ordre de 20 Go par mois pour les paquets quotidiens, plus environ 45 Go à chaque reconstruction des bases, **quel que soit le nombre d'utilisateurs** ; la sortie gratuite d'Oracle est de 10 To par mois.

## Stockage

| | 2 sites | 219 domaines |
|---|---|---|
| Fichiers publiés (`public/` : base, paquets, index) | 82 Mo | ~11 Go (≈ 50 Mo par domaine) |
| Contenus internes (`blobs/`, partagés entre sites : polices, sprites, styles) | 79 Mo | ~8 Go |
| Pipeline : sorties (`out/`) | 0,6 Go | ~55 Go |
| Pipeline : cache (dalles LiDAR, tuiles IGN) | 5 Go | ~590 Go : trop pour le volume gratuit du serveur (200 Go en tout) |

À 219 domaines, les constructions lourdes (première construction, relief, exposition aux avalanches) se font sur le poste, qui garde le cache ; le serveur ne garde que les sorties et le dépôt publié (~75 Go). Sinon : un volume de blocs en plus, environ 0,0255 $ par Go et par mois (≈ 2,6 $ pour 100 Go).

Calcul quotidien pour 219 domaines : 3,3 s par domaine sur le poste, deux à trois fois plus sur les 4 cœurs du serveur, soit environ 30 à 40 minutes par jour en CronJob. Un `build-all` complet de 219 domaines prend environ 9 h sur le poste (150 s par domaine) et plus d'une journée sur le serveur : il n'est pas hebdomadaire à cette échelle.

## Où servir : origine serveur ou R2

Le modèle `Worker PMTiles` **ne s'appuie pas sur R2** : c'est un Worker qui lit les PMTiles sur l'origine (serveur, derrière Cloudflare Access) par requêtes partielles et découpe les tuiles (quota gratuit des Workers : 100 000 requêtes par jour). skimap n'a pas besoin de Worker : le téléphone télécharge des paquets entiers, servis tels quels.

| | Origine serveur + cache Cloudflare (déployé) | R2 (bucket public sur un sous-domaine, même cache) |
|---|---|---|
| Stockage | gratuit (volume du serveur) | 10 Go gratuits, puis 0,015 $ par Go et par mois |
| Opérations | — | écriture (classe A) : 1 M gratuites puis 4,50 $ par million ; lecture (classe B) : 10 M gratuites puis 0,36 $ par million ; seuls les défauts de cache atteignent R2 |
| Sortie | 10 To gratuits par mois chez Oracle, cache Cloudflare gratuit | gratuite |
| 2 sites | 0 € | 0 € |
| 219 domaines | 0 € | ≈ 0,02 $ par mois (11 Go) ; écritures ≈ 50 000 par mois : gratuites |
| 100 000 utilisateurs | 0 €, mais risque : les conditions du CDN gratuit de Cloudflare restreignent la diffusion d'une « part disproportionnée » de gros fichiers hors HTML ; R2 y est explicitement autorisé | ≈ 0 à 0,10 $ par mois |

Tarifs de R2 tels que publiés par Cloudflare (connus en 2026, à revérifier au moment de l'activer) ; R2 demande d'enregistrer un moyen de paiement même dans le palier gratuit, d'où une « Demande à l'orchestrateur ». Le dépôt publié est un dossier de fichiers statiques (`public/`) : le copier tel quel dans un bucket suffit, avec `catalog.json` et les index envoyés en dernier.

## Autres services

- E-mail des alertes (S2) : relais SMTP d'un compte Gmail gratuit, 0 € (500 destinataires par jour).
- Strava (S1) : API gratuite ; l'app Strava est limitée à 1 puis 10 athlètes avant une revue.
- Domaine : `warnier.net` existant ; Cloudflare Free ; ArgoCD, registre et buildkitd du cluster existants.

## Total mensuel

- **2 sites, usage familial : 0 €.**
- **219 domaines, jusqu'à 100 000 utilisateurs : 0 € à environ 0,10 $** (R2 conseillé au-delà de quelques milliers d'utilisateurs), plus environ 2,6 $ si l'on garde le cache du pipeline des 219 domaines sur le serveur au lieu du poste. Aucun coût par utilisateur.
