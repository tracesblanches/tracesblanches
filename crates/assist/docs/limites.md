# Limites communes

[← skimap-assist](../README.md)

- Météo d'un seul point par secteur (2 à Cervières, 1 à Chapelle-des-Bois) : les domaines voisins prennent le plus proche (jusqu'à 10 km, dit dans la raison).
- La carte d'enneigement P6 ne couvre que l'emprise du site : Val-des-Prés (Névache) et les parties hors emprise ont une neige « inconnue ». Elle sous-estime de 10 cm en 2025-26 à Cervières (P7) et dure trop longtemps dans le Jura en février.
- État des pistes : seulement depuis le premier relevé (14/11/2026) ; les jours passés n'ont pas d'état. Nordic France ne publie pas de géométrie (rapprochement par le nom dans la couche `status`) ; « Liaison les Aittes par la carrière », « Liaison les Aittes par la route », « Piéton les Fonts » et « Piéton les Oules » n'ont pas de way OSM du même nom (`stats.join` de `status.meta.json`).
- L'humidité n'est pas dans la couche P19 (seulement dans les fichiers d'historique).
- Le BERA du Queyras n'est lisible hors ligne que par l'API (clé) ou l'archive XML jusqu'en avril 2023 ; depuis, l'archive n'a que le PDF : la synthèse le dit.
- Distance à vol d'oiseau, pas par la route.
