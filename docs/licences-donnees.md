# Licences des données et du code

Recherche documentaire du 15/11/2026 (tickets L1 #6, L2 #7, L3 #8, L4 #9, L5 #10, L6 #11, F2 #2). Ce n'est pas un avis juridique : chaque conclusion cite le texte lu, et ce qui n'a pas pu être lu est marqué **non vérifié**.

## Sommaire

1. [Sources (1/2) : cartes, relief, domaines, zones](licences/sources-cartes-zones.md)
2. [Sources (2/2) : neige, météo, avalanches, état des pistes, refuges, fartage](licences/sources-neige-etat-refuges.md)
3. [L3 : combiner ODbL, CC BY-SA et Licence Ouverte](licences/l3-combinaison.md)
4. [F2 : licence du code, code et modèles tiers, licence des données produites](licences/f2-licence-code.md)
5. [L2 : IGN hors ligne (Plan IGN v2, carte des pentes)](licences/l2-ign.md)
6. [L5 : Strava (OAuth, import, clauses de l'API)](licences/l5-strava.md)
7. [Courriers à envoyer : ENJ, Nordic France, DDT des Hautes-Alpes](licences/courriers.md)

## En bref

- **Libres pour un pack hors ligne, y compris dans une app payante, avec attribution** : OSM et ses dérivés (OpenSnowMap, OpenSkiData, Protomaps : ODbL), Plan IGN v2, LiDAR HD MNT et MNH, BD Forêt (Licence Ouverte), Météo-France (simulation nivologique, BDClim, archive des BERA, paquets AROME : Licence Ouverte 2.0), ARCEP, GPU, DATAtourisme (Licence Ouverte), SkiRadar (attribution « skiradar.fr »), districts francs suisses (« terms_by »), refuges.info (CC BY-SA 2.0), Camptocamp collaboratif (CC BY-SA 3.0, coordonnées aussi en ODbL).
- **Partage à l'identique** : ODbL (toute base dérivée diffusée reste ODbL, art. 4.4) et CC BY-SA (toute adaptation reste CC BY-SA). Les deux sont incompatibles entre elles : jamais de base qui mélange OSM et refuges.info / Camptocamp pour un même type d'objet (schéma concret en L3).
- **Usage personnel seulement, tant qu'il n'y a pas d'accord écrit** : ENJ (mentions légales : tous droits « affichés ou en bases de données »), Nordic France (« Toute reproduction […] est interdite, sauf autorisation écrite préalable »), pages « Info damage » des mairies, CLPA et EPA d'INRAE (« usage personnel excluant tout usage […] d'information »), Biodiv'sports (aucune licence trouvée).
- **Non commercial** : API gratuite d'Open-Meteo (une app payante est un « Commercial product ») ; Strava interdit de faire payer ce qui touche à son API (Policy 5.8).
- **Relu le 17/11/2026 (#171)** : carte des pentes IGN `SLOPES.MOUNTAIN` : Licence Ouverte par défaut des CGU de cartes.gouv.fr (aucune licence renseignée) ; INPN : Licence Ouverte 2.0 (Natura 2000 `fr-lo` du MNHN, espaces protégés `lov2` du Système d'information sur la biodiversité, `other-open` du MNHN) ; GPU : Licence Ouverte (CGU du GPU) ; contours des communes de geo.api.gouv.fr : **ODbL** (jeu « Contours administratifs », Admin Express et OSM). Détail et citations : [sources (1/2)](licences/sources-cartes-zones.md), mise en œuvre : [L3](licences/l3-combinaison.md#licences-vérifiées-et-décision-unique-171-17112026).
- **À confirmer** : droit de la DDT 05 de republier la CLPA en Licence Ouverte (courrier 3).
- **L2** : le Plan IGN v2 est en Licence Ouverte (fiche IGNF_PLAN-IGN), qui permet de « redistribuer » et d'inclure les données « dans son propre produit ou application » ; les CGU de cartes.gouv.fr (15/10/2024) ne disent rien du cache et limitent la TMS à 400 requêtes/s. Le pack hors ligne est donc permis ; la mention doit porter la date de mise à jour.
- **L5** : la documentation Strava ne mentionne pas PKCE ; l'échange du code et le rafraîchissement exigent `client_secret` (« required »), y compris pour l'OAuth mobile.
- **F2** : décision du 15/11/2026 : code sous **GPL-3.0-or-later** avec une permission additionnelle pour les boutiques d'applications (`LICENSE`, `LICENSE-EXCEPTION`), `crates/avalanche` (portage de com4FlowPy) sous **EUPL-1.2**, jamais liée par l'app ; données produites : chaque couche sous la licence de sa source, nos couches propres sous Licence Ouverte 2.0.

## Décision L3 (note de décision du ticket #8)

1. Une couche par type d'objet et par source, jamais de fusion OSM + CC BY-SA. Le pack est une base collective (ODbL art. 4.5 a, ligne directrice OSMF « Collective Database ») et une œuvre collective au sens CC BY-SA (« Collective Work »).
2. Refuges et abris : refuges.info est la **seule** source des géométries dans un pack public ; Camptocamp ajoute des propriétés par identifiant ; les deux peuvent aller dans une seule couche sous CC BY-SA 4.0 (la clause « later version » de BY-SA 2.0 et 3.0 le permet). Les abris OSM (`refuges-osm`) et `links.json` restent en usage personnel ; les abris sont retirés du fond OSM du pack.
3. Pistes, chemins, domaines, graphe de routage : OSM seul (ODbL), code du pipeline publié (offre de l'art. 4.6 b). L'état des pistes ENJ / Nordic France, s'il est autorisé, est ajouté comme propriété « toute non-OSM » par référence, sans géométrie concurrente.
4. Licence Ouverte et CC BY 4.0 : permissives, elles entrent dans n'importe quelle couche avec leur attribution.
5. Le manifeste du pack liste les attributions (`attributions[]`, déjà présent) ; un dossier `LICENCES/` du pack contient les textes ; l'app affiche « © OpenStreetMap » dans un coin de la carte et les autres mentions dans un écran « Licences des données ».

## Ce qui reste ouvert

- Réponses écrites de l'ENJ (L1), de Nordic France (L4) et de la DDT 05 (CLPA) : courriers prêts.
- Biodiv'sports et INPN (L6) : écrire à la LPO et à PatriNat (courriers non rédigés : non demandés).
- Licence de Skimap.org (source d'OpenSkiData) et de swissBOUNDARIES3D : non vérifiées.
- CGU du portail API Météo-France (BERA par API, AROME WCS) : page rendue en JavaScript, illisible ici ; l'archive des BERA et les paquets AROME sont en Licence Ouverte 2.0 sur data.gouv.
- Theia Neige (L6) : la page produit annonce « ETALAB V2.0 » pour les données Sentinel-2 niveau 2A/3A ; pour le produit neige lui-même, non confirmé (Theia n'est utilisé nulle part dans le code).
