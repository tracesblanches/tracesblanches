# Jeu de données de référence des tests : contenu et licences

Extrait des données complètes des deux sites (Cervières, Chapelle-des-Bois) par `skimap fixtures` (ticket Q1, #101), pour que `cargo test --workspace` couvre les deux sites sans réseau. `manifest.json` liste chaque fichier avec sa taille, son SHA-256, sa clé de licence (ci-dessous) et son origine ; un test vérifie le manifeste, la présence de chaque clé dans ce fichier et l'absence des sources exclues. Analyse des licences : `docs/licences-donnees.md`. Ce n'est pas un avis juridique.

## Régénérer

```sh
cargo run --release -p skimap-cli -- fixtures            # data/ complet requis (cache rempli, build-all fait)
cargo test -p skimap-cli --test fixtures -- --ignored full_data_   # le jeu versionné est-il à jour ?
```

Avant d'écrire, le générateur prouve la fidélité de chaque extrait : couche `pistes` reconstruite depuis l'extrait OSM réduit et graphe `piste_graph.json` reconstruit depuis le MNT réduit identiques octet pour octet à `data/out` ; pente et orientation de la fenêtre identiques à la couche `terrain`. Deux exécutions donnent les mêmes octets.

## Contenu

- `inputs/osm/planet_pistes_sites.osm.gz` : extrait OpenSnowMap réduit aux ways de la région que regarde le constructeur de pistes autour de chaque site (avec tous leurs nœuds et les relations qui les atteignent).
- `inputs/<site>/dem_profile_cells_5m.tif` : MNT LiDAR HD 5 m réduit aux cellules lues par les profils d'altitude du graphe (NaN ailleurs).
- `inputs/<site>/dem_window_5m.tif` : fenêtre de 1 km × 1 km du MNT (la plus grande variété de pentes).
- `golden/` : empreintes chiffrées de référence (ticket Q2, #102), chiffres et phrases calculés par les tests sur ce jeu (`golden/full/` : sur les données complètes) ; mises à jour seulement avec `SKIMAP_BLESS=1`.
- `data/out/<site>/<couche>/` : sorties du pipeline lues par les crates du téléphone (pistes, graphe, zones, couverture, refuges, neige, exposition de Chapelle-des-Bois) ; les `.meta.json` modifiés le disent dans `notes`.

## Clés de licence

| Clé | Sources | Licence et attribution |
|---|---|---|
| `odbl-osm` | OpenSnowMap `planet_pistes.osm.gz` (extrait d'OSM), couches `pistes`, graphe `routes/piste_graph.json`, `refuges/osm.geojson` | ODbL 1.0 : « © les contributeurs d'OpenStreetMap », extrait quotidien OpenSnowMap.org ; toute base dérivée diffusée reste sous ODbL. Les altitudes du graphe viennent du MNT IGN (`lo-ign`). |
| `lo-ign` | MNT LiDAR HD (IGN), `terrain.meta.json` | Licence Ouverte 2.0 (Etalab) : « IGN, LiDAR HD MNT », date d'édition dans les métadonnées. |
| `lo-meteofrance` | couche `snow` (simulation nivologique Météo-France, croisée avec le relief IGN) | Licence Ouverte 2.0 : « Météo-France, données de modèle de simulation nivologique » ; relief : `lo-ign`. |
| `lo-arcep` | couche `coverage` (cartes Mon Réseau Mobile, partie française par les contours de geo.api.gouv.fr) | Licence Ouverte : « ARCEP, Mon Réseau Mobile », publication dans les métadonnées ; contours : voir `zones-mixed`. |
| `cc-by-sa-refuges` | `refuges/refuges_info.geojson`, `refuges/camptocamp.geojson`, `refuges.meta.json` | CC BY-SA 2.0 (« ©Les contributeurs de Refuges.info ») et CC BY-SA 3.0 (documents collaboratifs Camptocamp, avec contributeurs, lien et licence) ; partage à l'identique. Jamais mélangés avec OSM (décision L3). |
| `zones-mixed` | couche `zones` sans Biodiv'sports | INPN / PatriNat (data.gouv.fr : « other-open », texte exact non vérifié) ; Géoportail de l'urbanisme (Licence Ouverte) ; geo.admin.ch, districts francs OFEV (« terms_by » : « © OFEV », « © swisstopo ») ; geo.api.gouv.fr (IGN Admin Express, Licence Ouverte ; API sans licence affichée, non vérifié). |
| `lo-skimap` | nos couches propres : `zones/rules.json` (règles écrites par le projet, chaque règle cite son texte officiel), exposition aux avalanches de Chapelle-des-Bois (calcul sur le MNT et le MNH IGN) | Licence Ouverte 2.0 (décision F2 : nos couches propres) ; sources : `lo-ign`, pistes `odbl-osm`. |

## Exclus (jamais dans ce dossier)

- Sources à usage personnel (`personal_sources`) : widget Nordic France, page « Info damage » de la mairie de Cervières, état des pistes ENJ, API gratuite d'Open-Meteo (dont l'historique météo de l'assistant) ; donc aucune couche `status`, `meteo-hourly`, `assist`.
- CLPA : réservée à l'usage personnel par INRAE ; la republication de la DDT 05 attend la réponse au courrier 3 : pas de couche `avalanche-historic`, pas d'exposition de Cervières (elle intègre la CLPA).
- Biodiv'sports : aucune licence trouvée ; ses zones sont retirées de `zones.geojson` et de `zones.meta.json`.
- Données personnelles de tiers : les couches de refuges ne gardent que les champs utiles aux tests (géométrie, nom, type, capacité, équipements) ; commentaires, auteurs, remarques, accès, propriétaires, téléphones, e-mails et sites web sont retirés, ainsi que les étiquettes de contact et de texte libre de l'extrait OSM. Le générateur et un test refusent tout texte contenant un e-mail ou un numéro de téléphone (`support/privacy.rs`).
- `refuges/links.json` (liens entre sources de licences incompatibles, usage personnel selon L3) ; `snow/massif.tif` (contours snowtools, CeCILL-C) ; BERA (bulletin daté, inutile aux tests).
- Fonds de carte, clips de la voix, rasters du hors piste, ARCEP nationaux : trop gros ; les tests qui en ont besoin sont dans la catégorie « données complètes » (`full_data_`, tâche hebdomadaire).
