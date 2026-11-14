# Visualiseur des packs (P15, #28)

Page statique `index.html`, sans étape de construction : MapLibre GL JS 5.24.1 et le client PMTiles 4.5.0 sont chargés depuis jsDelivr (versions figées, empreintes SRI). Toutes les données viennent du pack, servi en local : fonds de carte (Plan IGN v2, OpenStreetMap Protomaps), couches GeoJSON, rasters en tuiles, polices et icônes. Après un premier chargement, le navigateur garde les deux scripts en cache : la carte s'affiche sans réseau.

## Ouvrir

Les PMTiles se lisent par requêtes partielles (`Range`) : `python3 -m http.server` les ignore et le client PMTiles refuse alors l'archive. Le serveur de la crate `pack` les gère (bibliothèque standard seulement) :

```sh
# depuis la racine du dépôt (data/ doit y être, ou un lien symbolique vers lui)
cargo run --release -p skimap-pack --features pipeline --example serve -- . 127.0.0.1:8765
google-chrome "http://127.0.0.1:8765/tools/viewer/index.html?pack=../../data/out/cervieres/pack/"
```

Le menu en haut du panneau passe d'un site à l'autre (Cervières, Chapelle-des-Bois) ; `?pack=<chemin>` ouvre n'importe quel pack servi par le même serveur.

## Sur le Pixel (Chrome Android)

Téléphone branché en USB, débogage autorisé, serveur lancé comme ci-dessus :

```sh
adb reverse tcp:8765 tcp:8765
```

puis, dans Chrome sur le téléphone : `http://localhost:8765/tools/viewer/index.html?pack=../../data/out/cervieres/pack/`. Rien n'est exposé sur le Wi-Fi. Le panneau est replié sur petit écran (bouton ☰) ; les cases, boutons radio et curseurs ont une taille tactile ; la géolocalisation (bouton ◎) marche sur `localhost`.

## Ce qu'il montre

- **Fond de carte** : Plan IGN v2 ou OpenStreetMap (Protomaps), ou aucun.
- **Couches** : toutes celles du manifeste, à cocher ; les rasters (pentes LiDAR HD, pentes IGN pour comparer, neige estimée et ses classes) avec légende et curseur d'opacité.
- **Fiches** : toucher un objet ouvre une fiche, avec un format dédié pour les pistes (difficulté, damage, longueur, itinéraires), les zones (type, mois sensibles, **règles** en clair), les refuges (état, places, **dernier commentaire daté**), les tronçons en zone d'avalanche (**longueur dans l'emprise**, texte d'alerte), les zones sans réseau, l'état des pistes ENJ et les itinéraires de preuve ; les autres couches affichent leurs propriétés brutes.
- **Infos du pack** : validité du pack et de chaque couche, BERA (ou message « pas de BERA » pour le Jura), synthèse des avalanches historiques, état des pistes, voix.
- **Attributions** : celles du manifeste, puis chaque source avec sa licence et la mention « usage personnel » quand elle s'applique.
- **Couches absentes** : ce que le pack n'a pas trouvé et pourquoi.

Vérification faite (Chrome 154 sans tête, protocole DevTools) : la liste des requêtes ne contient que `127.0.0.1` (pack) et `cdn.jsdelivr.net` (les deux scripts et la feuille de style), aucune erreur dans la console hormis `favicon.ico`.
