# L3 : combiner ODbL, CC BY-SA et Licence Ouverte dans une seule app

Ticket #8. Retour au [sommaire](../licences-donnees.md). Textes lus le 15/11/2026.

## Ce que disent les textes

**ODbL 1.0** (https://opendatacommons.org/licenses/odbl/1-0/) :
- 4.4 a : « Any Derivative Database that You Publicly Use must be only under the terms of: i. This License; ii. A later version […]; or iii. A compatible license. » ; 4.4 d : « You must not add Contents to Derivative Databases under Section 4.4 a that are incompatible with the rights granted under this License. »
- Définition : « Collective Database – Means this Database in unmodified form as part of a collection of independent databases in themselves that together are assembled into a collective whole. A work that constitutes a Collective Database will not be considered a Derivative Database. »
- 4.5 a : « You are not required to license Collective Databases under this License if You incorporate this Database or a Derivative Database in the collection, but this License still applies to this Database or a Derivative Database as a part of the Collective Database » ; 4.5 b : « Using this Database, a Derivative Database, or this Database as part of a Collective Database to create a Produced Work does not create a Derivative Database ».
- 4.6 : qui diffuse une base dérivée ou une œuvre produite à partir d'elle doit offrir « a. The entire Derivative Database; or b. A file containing all of the alterations […] or the method of making the alterations to the Database (such as an algorithm) », gratuitement sur internet.

**Lignes directrices de l'OSMF** :
- « Collective Database Guideline », approuvée par le conseil le 17/06/2016 (https://osmfoundation.org/wiki/Licence/Community_Guidelines/Collective_Database_Guideline_Guideline) : OSM et non-OSM sont indépendants « so long as the data used for a particular data type is either all OSM or all non-OSM within the same regional cut », notamment quand « a non-OSM database replaces or adds a property of a primary feature, and uses either all OSM data or no OSM data for that property » ; une référence par clé est admise ; « Two data sets need not be physically separated ». Contre-exemple : compléter sa liste avec OSM « removing any duplicate objects » n'est pas couvert.
- « Horizontal Map Layers », approuvée le 06/06/2014 (https://osmfoundation.org/wiki/Licence/Community_Guidelines/Horizontal_Map_Layers_-_Guideline) : « if all data for that Feature Type is from non-OpenStreetMap sources, then the ODbl share-alike conditions do not apply to that Feature Type » ; mais « If you use OpenStreetMap data along with non-OpenStreetMap data for a given Feature Type, then the share-alike condition would apply regardless of whether some data for that Feature Type is in a different layer ». Exemple admis : fond OSM dont on exclut « ALL restaurants », puis sa propre couche de restaurants.
- « Substantial », approuvée le 06/06/2014 : non substantiel = moins de 100 objets, extraction unique ; « village map OK, town map not OK ». Nos extraits par site (des centaines de tronçons de pistes) sont substantiels.

**CC BY-SA** :
- 2.0 (https://creativecommons.org/licenses/by-sa/2.0/legalcode), 4(b) : une œuvre dérivée se diffuse seulement « under the terms of this License, a later version of this License with the same License Elements as this License, or a Creative Commons iCommons license » ; une « Collective Work » (œuvre « in its entirety in unmodified form » réunie à d'autres « separate and independent works ») « will not be considered a Derivative Work ».
- 3.0 (https://creativecommons.org/licenses/by-sa/3.0/legalcode), 4(b) : même règle (« (ii) a later version of this License with the same License Elements ») ; « Collection » n'est pas une « Adaptation ».
- Liste officielle de compatibilité (https://creativecommons.org/compatible-licenses/) : « Currently, no non-CC licenses have been designated as compatible with BY-SA 3.0 » ; pour 2.0 : seulement la même licence ou une version ultérieure. L'ODbL n'y figure pas.
- 4.0 (https://creativecommons.org/licenses/by-sa/4.0/legalcode.fr), article 4 : couvre explicitement le « Droit sui generis des producteurs de bases de données ». Les versions 2.0 et 3.0 n'en parlent pas.

**Licence Ouverte 2.0** et **CC BY 4.0** : attribution seule, pas de partage à l'identique ; la LO se dit compatible avec CC-BY et ODC-BY. Une donnée LO ou CC BY peut entrer dans une base ODbL comme dans une couche CC BY-SA.

## Conséquence

Une base qui mêle, pour un même type d'objet, des données OSM et des données refuges.info ou Camptocamp devrait être à la fois sous ODbL (4.4 a, 4.4 d) et sous CC BY-SA (4(b)) : c'est impossible. Il faut donc que chaque type d'objet vienne entièrement d'une famille de licences dans l'emprise d'un pack. Afficher plusieurs couches sur l'écran est une œuvre produite (4.5 b), mais la ligne directrice « Horizontal Map Layers » dit que le partage à l'identique revient dès que deux sources se complètent pour un même type d'objet (déduplication, « ceux qui manquent à OSM »). C'est le cas aujourd'hui des abris : `refuges-osm`, `refuges-info`, `refuges-camptocamp` et `refuges-links` sont livrés ensemble (crates/pack/docs/format/couches.md, table des couches reprises), et `links.json` est construit par l'étiquette OSM `ref:refuges.info` puis par position et nom.

## Schéma retenu pour un pack public

| Type d'objet | Source unique dans l'emprise | Couche et licence | Propriétés ajoutées par référence |
|---|---|---|---|
| Pistes nordiques, chemins, raquettes, domaines, graphe de routage | OSM (OpenSnowMap, Overpass, OpenSkiData) | `pistes`, `routes`, `domaines` : ODbL | état ouvert / fermé et date de damage (ENJ, Nordic France) si accord écrit : propriété « toute non-OSM » rattachée par identifiant de piste, aucune géométrie ENJ concurrente |
| Fond de carte OSM | Protomaps | `basemap/osm.pmtiles` : ODbL, abris (`alpine_hut`, `wilderness_hut`, `shelter`) retirés à l'extraction ou au style (présence de ces types dans les tuiles **non vérifiée**) | — |
| Refuges, cabanes, abris | refuges.info | une couche « refuges » sous CC BY-SA 4.0 (version ultérieure permise par BY-SA 2.0 et 3.0) | capacité hors gardiennage, gardiennage, matériel : Camptocamp (documents collaboratifs seulement), lien refuges.info ↔ Camptocamp calculé par position et nom, **sans** passer par OSM |
| Zones, règles, réseau, relief, neige, avalanche | sources LO (INPN sous réserve, GPU, DDT 05, IGN, Météo-France, ARCEP) | une couche par thème ; nos couches propres sous LO 2.0 | — |
| Météo horaire | Open-Meteo (CC BY 4.0) ou AROME (LO) | `meteo-hourly` | — |

Règles de mise en œuvre :
1. Abris OSM (`refuges-osm`) et `links.json` : construits seulement avec `personal_sources` (usage personnel, non publiés), puisqu'ils feraient « interagir » OSM et refuges.info.
2. Camptocamp : n'importer que les documents dont `license.id` vaut `CC-BY-SA-3.0` ; garder pour chaque propriété le document source et ses auteurs (crédit exigé par Camptocamp).
3. Pistes ODbL : l'offre de l'art. 4.6 b est tenue en publiant le dépôt (le code du pipeline est « the method of making the alterations ») ; écrire dans le pack l'URL du dépôt et la date de l'extrait OSM.
4. Attribution dans l'app : « © OpenStreetMap » dans un coin de la carte (masquable après 5 s ou au premier geste, OSMF) ; « Refuges : © contributeurs de refuges.info, camptocamp.org (CC BY-SA 4.0) » quand la couche est visible ; fiche d'un refuge : auteurs Camptocamp et lien vers le document, auteur et date du dernier commentaire refuges.info ; écran « Licences des données » : toutes les lignes de `attributions[]` et les textes de `LICENCES/`.
5. Le pack lui-même (dossier de couches indépendantes) est une base collective (ODbL 4.5 a) et une œuvre collective (CC BY-SA « Collective Work ») : chaque couche garde sa licence, le pack n'en a pas d'unique.

## Mise en œuvre (#169, 17/11/2026)

Une seule décision par licence, `skimap_common::licence` : famille ODbL, attribution seule (Licence Ouverte, CC BY, CC0, ODC-BY), CC BY-SA, ou personnelle (aucune licence de réutilisation connue, ou source marquée `personal_use_only`), lue au début du texte de licence publié par la source. Le graphe de routage (`skimap_osm::srn_ways`), les départs des itinéraires (`skimap_pack::build::starts`) et les couches du pack s'en servent.

- Itinéraires et zones SRN : une couche par groupe de licences de leurs entités (`skimap_pack::build::layers::srn_specs`) : `srn-routes` / `srn-areas` (ODbL et attribution seule), `srn-routes-cc-by-sa` / `srn-areas-cc-by-sa` (Camptocamp, à part, avec ses seules sources et son attribution), `srn-routes-personnel` / `srn-areas-personnel` (instances Geotrek sans licence affichée, et zones qui mêlent itinéraires ODbL et CC BY-SA) seulement avec `personal_sources`. Chaque couche a ses métadonnées (`layers/meta/srn/srn<suffixe>.meta.json`) avec les seules sources de son groupe, sans les statistiques de la couche France (qui nomment des itinéraires de toutes les sources).
- Refuges : règle 1 appliquée, `refuges-osm` et `refuges-links` seulement avec `personal_sources` ; chaque couche de refuges ne garde que les sources de la licence de son fichier (`files[].licence` des métadonnées).
- L'écran des sources du téléphone (ⓘ, `CreditsDialog`) liste les attributions des sources de chaque couche dessinée (`skimap_ffi::style`) : celles de Camptocamp n'y figurent que si une couche CC BY-SA est présente.
