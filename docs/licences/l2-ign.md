# L2 : IGN hors ligne (Plan IGN v2, carte des pentes)

Ticket #7. Retour au [sommaire](../licences-donnees.md). Lu le 15/11/2026.

## Textes et métadonnées lus

1. **CGU de cartes.gouv.fr**, « Version du 15 octobre 2024 » (https://cartes.gouv.fr/cgu/ ; https://geoservices.ign.fr/cgu-licences y redirige) :
   - « L'accès à une API ne confère aucun droit de propriété intellectuelle sur l'API et sur les données mises à disposition via l'API. »
   - « Les données contenues dans les API sont soumises à la licence choisie et renseignée par le Fournisseur de données, accessible depuis les métadonnées de données de l'API. Vous vous engagez à respecter les termes de la licence attachée à ces données »
   - pour les fournisseurs de données : « A défaut, la Licence Ouverte / Open Licence – Etalab s'applique sur ce jeu de données. »
   - article 3.2 (fair use) : « API Géoplateforme - Diffusion de tuiles vectorielles TMS : 400 requêtes/s », « Diffusion d'images tuilées WMTS : Non soumis à limite d'usage », WMS-Raster 40 requêtes/s, WFS 30 requêtes/s ; au-delà, erreur 429 pendant 5 secondes.
   - aucune clause sur le cache, le préchargement ou le hors ligne : les recherches « cache », « hors ligne », « stock », « télécharg », « aspir » ne trouvent que le stockage du compte et l'API de téléchargement.
2. **GetCapabilities WMTS** (https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetCapabilities) : `AccessConstraints` « Conditions Générales d'Utilisation disponibles ici : https://cartes.gouv.fr/cgu », `Fees` « none ». La couche `GEOGRAPHICALGRIDSYSTEMS.PLANIGNV2` renvoie à la fiche `IGNF_PLAN-IGN`.
3. **Fiche IGNF_PLAN-IGN** (CSW `https://data.geopf.fr/csw`, `GetRecordById`) : « Licence Ouverte / Open License (compatible ODC-BY, CC-BY 2.0) », « Pas de restriction d'accès public selon INSPIRE », dates « 2024-12-01 publication » et « 2026-10-08 revision ».
4. **TMS vectoriel** `https://data.geopf.fr/tms/1.0.0/PLAN.IGN/metadata.json` : « Plan IGN personnalisable », ni licence ni lien vers une fiche. Le style `standard.json` (`data.geopf.fr/annexes/ressources/vectorTiles/styles/PLAN.IGN/`) n'affiche pas de licence.
5. **Carte des pentes** : couche WMTS `GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN`, titre « Carte des pentes », résumé « Carte des zones ayant une valeur de pente supérieure à 30°-35°-40°-45° d'après la RGE ALTI® au pas de 5m », **aucune MetadataURL** ; la recherche `*pente*` dans le catalogue CSW ne renvoie que quatre fiches sans rapport (forêts, ENR). Fiche `IGNF_RGE-ALTI` : Licence Ouverte.
6. **LiDAR HD** : fiches `IGNF_MNT-LIDAR-HD` et `IGNF_MNH-LIDAR-HD` : Licence Ouverte.

## Réponse

- **Plan IGN v2 vectoriel : préchargement et redistribution dans un pack hors ligne permis**, y compris dans une app payante. Base : la Licence Ouverte de la fiche IGNF_PLAN-IGN, qui autorise à « reproduire », « redistribuer » et à inclure l'information « dans son propre produit ou application » ; les CGU renvoient à cette licence et n'ajoutent aucune interdiction. Réserve : la fiche est rattachée à la couche WMTS ; que la TMS vectorielle (même produit, même nom) relève de la même fiche est une déduction, non écrite. Contrainte technique : rester sous 400 requêtes/s par IP lors de la construction des packs (le pipeline télécharge depuis le poste, une requête à la fois).
- À ne pas confondre avec le **SCAN 25**, qui n'est pas libre et dont la mise en cache est payante (rapport 03).
- **Style, sprite, glyphes** : licence non affichée pour le style et le sprite (**non vérifié**, même fournisseur, traités comme le Plan IGN) ; glyphes Source Sans, OFL-1.1.
- **`SLOPES.MOUNTAIN` : non confirmé.** Probablement Licence Ouverte (carte dérivée par l'IGN de la RGE ALTI, elle-même en Licence Ouverte ; défaut des CGU), mais aucun texte ne le dit pour cette couche. Décision proposée : ne livrer que nos classes de pente calculées sur le MNT LiDAR HD (Licence Ouverte), déjà dans le pack (couche `slope`, z10-15) ; garder les tuiles IGN (`SKIMAP_PACK_IGN_SLOPES=1`) pour la comparaison en usage personnel. Si l'on veut les tuiles IGN dans un pack publié : demander une réponse écrite via le formulaire de contact de cartes.gouv.fr.

## Mention d'attribution

La Licence Ouverte demande la source et « la date de dernière mise à jour », sans suggérer de caution. Proposition :
- coin de la carte : « © IGN – Plan IGN v2 » ;
- écran « Licences des données » : « Fond de carte : IGN, Plan IGN v2, Licence Ouverte Etalab 2.0, tuiles téléchargées le AAAA-MM-JJ. Relief : IGN, LiDAR HD MNT et MNH (éditions du 15/03/2025 pour le jeu de test alpin, du 21/07/2025 pour le jeu de test jurassien), Licence Ouverte 2.0. Pentes calculées par skimap à partir du LiDAR HD. »
- Corrigé le 15/11/2026 (commit 2013a5b) : le pack écrit « © IGN – Plan IGN v2, mise à jour du AAAA-MM-JJ (fiche IGNF_PLAN-IGN), tuiles téléchargées le AAAA-MM-JJ, Licence Ouverte Etalab 2.0 » dans le manifeste, le visualiseur et l'export.
