# L'écran carte (K15 #123, H2 #133, K12 #115)

[← skimap pour Android](../README.md)

## Styles validés, aucun style à l'app

- Les six fichiers de `styles/` (fond, SRN, guidage, clair et sombre) sont embarqués tels quels (`assets/styles/`, tâche Gradle `styleAssets`).
- Le noyau (`crates/ffi`, `style::instantiate`) réécrit leurs adresses relatives en `file://<pack>/…` et `pmtiles://file://<pack>/…`, refuse un chemin qui sort du pack, et retire les sources qu'un pack n'a pas (et leurs couches), listées dans `MapStyle.missing`.
- Le dessin (couleurs, épaisseurs, filtres, zooms, ordre des couches) est celui du fichier : le test `test_pack_with_every_validated_style` vérifie que chaque couche gardée est identique à l'originale, adresses mises à part.

## Choix du style sans manipulation

- Pratique choisie dans le menu, mémorisée : ski de fond sur pistes damées → `fond`, ski de randonnée nordique → `srn`.
- `guidage` pendant une journée planifiée : `MapViewModel.setGuiding(true)`, appelé par le guidage (agent B).
- Clair ou sombre selon le thème du système, sans second chargement au démarrage (thème lu par l'activité avant l'ouverture).

## Mode libre par défaut (H2)

- Tous les packs installés dans une seule carte. Sans couture : chaque couche du style est répétée juste après elle-même pour chaque pack (`~p1`, `~p2`…), donc toute forêt sous toute piste, toute piste sous toute étiquette ; fond et sources remplies par l'app (position, itinéraire, décision) une seule fois.
- La caméra reste au-dessus des packs ; au lancement, elle revient où l'utilisateur l'avait laissée (notée une fois, quand l'app passe en arrière-plan), sinon sur le premier pack.
- Mode domaine depuis le menu ou la carte de France ; « Vue auto » recentre sur le pack courant.

## Couches et légende

- Un interrupteur par groupe désactivable du style (`metadata.skimap:groups`, `toggleable`) : pentes, refuges, points d'eau, parkings et accueils, noms, zones sans réseau, neige (selon le style et ce que les packs contiennent).
- Les groupes de sécurité (pistes et état, dangers d'avalanche, zones réglementées, position) ne se masquent pas. La neige est masquée par défaut (`optional_layers`).
- Visibilité seule, sans recharger le style ; choix mémorisé par famille de style (`fond`, `srn`, `guidage`).
- Légende lue dans le style lui-même : couleurs de `pistes-line` (difficultés), pistes fermées, hors piste balisé, raquettes, classes de pente (`slope-*`) et d'exposition (`exposure-*`) ; un mot pour chaque couleur, en français ou en anglais.

## Commandes et fiches

- Gestes : pincement, double toucher, double toucher puis glisser. Aucun bouton de zoom ; « Vue auto » (48 dp) n'apparaît qu'après un geste.
- Menu « Carte et légende » et ⓘ en coins, petits, semi-transparents ; les crédits, licences et la version des packs sont derrière ⓘ.
- Toucher un refuge (ou une station) : fiche (feuille du bas) dans la langue de l'app ; les mots des sources restent tels quels. Téléphones à composer (le composeur s'ouvre, l'app n'appelle jamais), liens, source et date.
