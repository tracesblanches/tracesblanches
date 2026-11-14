# skimap-refuges

Refuges et abris : refuges.info, OSM, Camptocamp (pipeline). Ticket : P11 (#24).

## Sources

| Source | Accès | Licence | Ce qu'on garde |
|---|---|---|---|
| refuges.info | `/api/bbox?detail=complet&nb_points=all` (source `refuges-info`, 1 req/s) | CC BY-SA 2.0 | type, état, places, matelas, poêle, cheminée, eau, couvertures, latrines, mur manquant, accès, remarques, date de modification |
| refuges.info, commentaires | `/api/commentaires?id_point=a,b,c` (40 points par requête) | CC BY-SA 2.0 | dernier commentaire daté de chaque point (date, texte, auteur, photo) |
| OpenStreetMap | Overpass public (source `overpass`, 1 requête / 5 s), une requête par site : `tourism=alpine_hut`, `tourism=wilderness_hut`, `amenity=shelter`, `ref:refuges.info` | ODbL | type (`kind`), `ref:refuges.info`, étiquettes utiles (capacité, `winter_room`, cheminée, horaires…) |
| Camptocamp | `/waypoints?wtyp=hut,shelter,gite&bbox=` (EPSG:3857) puis `/waypoints/<id>?cook=fr` | CC BY-SA 3.0 (licence par document) | capacité hors gardiennage (`capacity`), gardée, gardiennage, matelas, couvertures, gaz, chauffage hors gardiennage, période de gardiennage, auteurs |
| geo.api.gouv.fr | `communes?lat=&lon=` | Licence Ouverte 2.0 | `in_france` (les emprises débordent sur l'Italie et la Suisse) |

Mise à jour de refuges.info : l'instantané complet de l'emprise est gardé 7 jours (politique du téléchargeur) ; chaque construction demande en plus les points modifiés depuis cet instantané (`depuis=<date unix>`, gardé 1 h) et les pose par-dessus. Un nouvel instantané chaque semaine rattrape les suppressions. Les commentaires sont gardés 1 jour. Vérification à la main de `depuis` : sur Cervières, `depuis` = maintenant − 30 jours renvoie 7 points, dont deux dont la fiche date de 2023 et 2024 ; un nouveau commentaire compte donc comme une modification.

## Licences et couches

Les trois sources restent dans trois fichiers ; aucune base fusionnée n'est produite tant que la compatibilité CC BY-SA / ODbL (ticket L3, #8) n'est pas tranchée. `links.json` ne contient que des identifiants : quels enregistrements décrivent le même lieu.

- OSM → refuges.info : étiquette `ref:refuges.info` (exact) ;
- sinon par position et nom : enregistrement le plus proche à moins de 250 m dont le nom partage des mots (Jaccard ≥ 0,3, mots génériques « refuge », « cabane », « abri »… retirés ; la ressemblance du nom passe avant la distance), ou à moins de 40 m quel que soit le nom ;
- Camptocamp est rattaché à refuges.info d'abord, sinon à OSM ;
- jamais par position : abribus (`shelter_type=public_transport`), points d'eau et passages délicats de refuges.info.

## Sorties

`data/out/<site>/refuges/` (WGS84) : `refuges_info.geojson`, `osm.geojson`, `camptocamp.geojson`, `links.json`, `refuges.meta.json` (sources avec URL, licence, attribution, ETag, sha256, dates ; propriétés documentées ; chiffres). Chaque point porte `in_france` et `link` (index dans `links.json`).

API : `skimap_refuges::build(site, &layout, &downloader)`. Exemple : `cargo run --release -p skimap-refuges --example refuges`.

## Chiffres au 14/11/2026 comparés au rapport 06

| | Cervières | Rapport 06 | Chapelle-des-Bois | Rapport 06 |
|---|---|---|---|---|
| refuges.info, total | 22 | 22 | 69 | 69 |
| cabanes non gardées (France / hors France) | 7 / 1 | 8 (7 en France) | 15 / 42 | 57, dont 42 hors France |
| refuges gardés / gîtes | 1 / 1 (Yourte des Ayes, « Fermé au public ») | 1 / 1 | 0 / 6 (dont « Chez L'Aimé » fermé) | 6 gîtes |
| points d'eau / autres | 10 / 2 bâtiments « Inutilisable » | 10 / 2 | 5 (4 + 1) / 1 passage délicat | 5 / 1 |
| avec un dernier commentaire daté | 21 | — | 62 | — |
| OSM `alpine_hut` (France / hors France) | 2 / 3 | 2 / 3 | 7 / 0 | 7 / 0 |
| OSM `wilderness_hut` | 2 / 1 | 2 / 1 | 4 / 2 | 4 / 2 |
| OSM `amenity=shelter` | 10 / 0, dont 5 abribus | 10 / 0, dont 5 abribus | 19 / 40, dont 4 abribus | 19 / 40 |
| OSM `ref:refuges.info` / `fireplace` / `winter_room` | 7 / 1 / 0 | 5 / 1 / 0 | 54 / 42 / 0 | 53 / 42 / 0 |
| Camptocamp hut / shelter / gîte | 3 / 1 / 1 | 3 / 1 / 1 | 1 / 20 / 2 | 1 / 20 / 2 |
| Camptocamp avec capacité hors gardiennage | 3 (Fonts de Cervières : 9 places, `always_accessible`) | Fonts : 9 places | 1 | — |
| lieux reliés (`links.json`) | 9 : 7 par `ref:refuges.info`, Camptocamp 3 vers refuges.info et 2 vers OSM | — | 57 : 54 par `ref:refuges.info`, 3 par position ; 22 Camptocamp vers refuges.info | — |

Les 7 `ref:refuges.info` de Cervières contre 5 au rapport : deux éléments OSM portent la référence sans être des abris (point d'eau « Fontaine des Fonts de Cervières », `kind = other`). Le rapport 06 compte `opening_hours` sur les restaurants, que ce ticket ne garde pas.

Temps : premier passage 60 s (Cervières) et 87 s (Chapelle), presque tout en appels réseau (geo.api.gouv.fr pour `in_france`, 23 fiches Camptocamp, une reprise sur un 502 d'Overpass) ; second passage depuis le cache : 0,02 s pour les deux sites.

Limites : refuges.info n'a aucun champ « ouvert en hiver » ni « local d'hiver » ; Camptocamp ne donne la capacité hors gardiennage que pour 4 abris sur 28 ; `winter_room` n'est renseigné sur aucun élément OSM des deux sites.
