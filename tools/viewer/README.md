# Visualiseur des packs (P15, #28)

Page statique `index.html`, sans étape de construction : MapLibre GL JS 5.24.1 et le client PMTiles 4.5.0 sont chargés depuis jsDelivr (versions figées, empreintes SRI). Toutes les données viennent du pack, servi en local : fonds de carte (Plan IGN v2, OpenStreetMap Protomaps), couches GeoJSON, rasters en tuiles, polices et icônes. Après un premier chargement, le navigateur garde les deux scripts en cache : la carte s'affiche sans réseau.

## Ouvrir

Les PMTiles se lisent par requêtes partielles (`Range`) : `python3 -m http.server` les ignore et le client PMTiles refuse alors l'archive. Le serveur de la crate `pack` les gère (bibliothèque standard seulement) :

```sh
# depuis la racine du dépôt (data/ doit y être, ou un lien symbolique vers lui)
cargo run --release -p skimap-pack --features pipeline --example serve -- . 127.0.0.1:8765
google-chrome "http://127.0.0.1:8765/tools/viewer/index.html?pack=../../data/out/cervieres/pack/"
```

Le menu en haut du panneau passe d'un site à l'autre (Cervières, Chapelle-des-Bois) ; `?pack=<chemin>` ouvre n'importe quel pack servi par le même serveur. Autres paramètres : `style=fond|srn|guidage|plan-ign|osm|none`, `theme=clair|sombre`, `layers=none` (fond d'origine sans couche), `panel=0` (panneau replié), `focus=decision` (cadre la position et la prochaine décision) ; la page met `window.__skimapIdle` à `true` quand la carte est chargée (captures).

## Sur le Pixel (Chrome Android)

Téléphone branché en USB, débogage autorisé, serveur lancé comme ci-dessus :

```sh
adb reverse tcp:8765 tcp:8765
```

puis, dans Chrome sur le téléphone : `http://localhost:8765/tools/viewer/index.html?pack=../../data/out/cervieres/pack/`. Rien n'est exposé sur le Wi-Fi. Le panneau est replié sur petit écran (bouton ☰) ; les cases, boutons radio et curseurs ont une taille tactile ; la géolocalisation (bouton ◎) marche sur `localhost`.

## Ce qu'il montre

- **Style** : les trois styles par pratique de `styles/` (K15, #123) — fond (pistes damées), SRN (hors piste), guidage —, en clair ou en sombre, les mêmes fichiers que l'app ; ou, pour comparer, le style d'origine d'un fond du pack (Plan IGN v2, OpenStreetMap Protomaps) avec les couches du visualiseur par-dessus, ou aucun fond.
- **Couches** : avec un style par pratique, une case par couche du pack qu'il dessine (y compris les couches masquées par défaut, comme la neige), avec la légende des rasters ; avec un fond d'origine, toutes les couches du manifeste, à cocher, rasters avec légende et curseur d'opacité.
- **Guidage (démonstration)** : le style guidage montre le premier itinéraire de preuve du pack (couche `routes`), le skieur à 35 % du parcours (partie faite en gris) et le prochain carrefour du graphe des pistes avec le sens à prendre (« À droite dans 690 m »). Le bouton ◎ remplace la position de démonstration par celle du GPS, dessinée par le style (source `position`), comme dans l'app.
- **Fiches** : toucher un objet ouvre une fiche, avec un format dédié pour les pistes (difficulté, damage, longueur, itinéraires), les zones (type, mois sensibles, **règles** en clair), les refuges (état, places, **dernier commentaire daté**), les tronçons en zone d'avalanche (**longueur dans l'emprise**, texte d'alerte), les zones sans réseau, l'état des pistes ENJ et les itinéraires de preuve ; les autres couches affichent leurs propriétés brutes.
- **Infos du pack** : validité du pack et de chaque couche, BERA (ou message « pas de BERA » pour le Jura), synthèse des avalanches historiques, état des pistes, voix.
- **Attributions** : celles du manifeste, puis chaque source avec sa licence et la mention « usage personnel » quand elle s'applique.
- **Couches absentes** : ce que le pack n'a pas trouvé et pourquoi.

Vérification faite (Chrome 154 sans tête, protocole DevTools) : la liste des requêtes ne contient que `127.0.0.1` (pack) et `cdn.jsdelivr.net` (les deux scripts et la feuille de style), aucune erreur dans la console hormis `favicon.ico`.
