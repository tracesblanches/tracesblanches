# skimap-core, C7 : demi-tour

[← skimap-core](../README.md)

Limite de lumière (coucher ou fin du crépuscule civil, à moins de 90 s d'astral 3.2 sur les deux sites), moins une marge (30 min avant le crépuscule civil par défaut, soit environ le coucher en hiver), moins le temps de retour C5 × 1,2 : heure de demi-tour et marge (`status`). `out_and_back` donne le point le plus lointain d'un aller-retour sur un itinéraire. Exemple au 15/01/2027, départ 13:00 : village du jeu de test jurassien → Chalet Gaillard aller-retour possible, retour 17:15 pour une limite à 17:17 ; Le Laus → Brunissard par l'Izoard impossible (retour 20:03), demi-tour au km 10,2 sur 16,6 à 16:12.

Les éphémérides supposent un horizon dégagé : le relief et la forêt avancent la nuit, la marge en tient lieu.
