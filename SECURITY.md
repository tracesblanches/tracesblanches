# Politique de sécurité

Traces Blanches sert en montagne, parfois loin de tout réseau. Une faille peut toucher la carte, le guidage, l'alerte de non-retour ou les données d'un skieur.

## Signaler une faille

Signalez-la en privé, par le signalement privé de GitHub : onglet **Security** du dépôt, puis **Report a vulnerability**. N'ouvrez pas d'issue publique pour une faille.

Indiquez ce que vous avez trouvé, comment le reproduire, et le commit concerné. Le correctif sort avant la publication des détails.

## Versions suivies

Les correctifs vont dans `main`. Aucune autre branche n'est maintenue, et aucune version n'est encore publiée.

## Ce qui est concerné

- la lecture des packs et des fichiers téléchargés (GeoJSON, PMTiles, rasters, manifestes, paquets différentiels, clips audio) : `crates/pack`, `crates/ffi`, cibles de `fuzz/` ;
- le téléchargeur du pipeline (cache, quotas, clés d'API qui ne doivent apparaître dans aucun journal) : `crates/common/src/download` ;
- le serveur `skimap-server` : catalogue et paquets, alerte de non-retour (e-mails), échange de jetons Strava, signalements : `crates/server` ;
- l'app Android : position, trace, plan de sortie, mode groupe : `android/`.

## Ce qui ne l'est pas

- Une donnée fausse dans une source (piste mal tracée dans OpenStreetMap, refuge fermé) : ouvrez une issue avec le modèle « Données ou site ».
- Un appareil déjà compromis, une clé d'API divulguée par son propriétaire.

## Sécurité en montagne

Une erreur de pente, d'exposition aux avalanches ou de zone protégée n'est pas une faille informatique, mais elle compte autant. Signalez-la en public avec le modèle « Données ou site », avec le domaine et la position.
