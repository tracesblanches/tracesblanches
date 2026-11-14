# A3 — qualité de neige par heure et par versant (#90)

[← skimap-assist](../README.md)

**Heuristique, pas un modèle de manteau neigeux** (formules et constantes dans la doc du module) : indice d'énergie de surface `E = (1 − α)·G + H·T − L` par heure et par classe (plat, 8 orientations à 20° et 40°, comme la simulation de Météo-France), où `G` est le rayonnement sur la pente sous les nuages (module `sun`), puis un état de surface (eau liquide retenue et percolation, croûte de regel, froid stocké, neige fraîche, vent) et une qualité : fraîche, soufflée, ancienne sèche, transformée (décaille), humide, soupe, dure regelée, croûte de glace (après pluie). Le modèle repart de zéro après un trou de plus de 6 h dans la météo et demande 24 h d'historique. Non modélisés : ombre du relief et de la forêt, damage, transport par le vent.

Validation qualitative (la vraie validation est T5, #47) :

| Jour | Données | Ce que dit A3 | Référence |
|---|---|---|---|
| Cervières 05–07/01/2026 | froid sec, ciel clair, −15 à −3 °C | plat : neige ancienne froide et sèche ; seules les pentes sud à 40° ramollissent vers 12–13 h | station de Cervières (1 614 m) : 19–20 cm, pas de chute |
| Cervières 09/01/2026 | 4 cm dans la journée | neige fraîche partout | station : 19 cm le 08, 40 cm le 09, 47 cm le 10 |
| Cervières 15/04/2023 | +1 à +2 °C au Laus | regelée le matin partout ; ramollit E et SE vers 9 h, S vers 10 h, O vers 11 h, N vers 10 h (20°) ; lourde au sud dès 12–13 h ; Les Fonts (2 044 m) reste dure au plat jusqu'à 13 h | BERA du Queyras : « généralement sur fond dur ou crouté » |
| Chapelle-des-Bois 22–28/02/2026 | +4 à +15 °C, pluie le 23 | transformée humide puis mouillée chaque jour, regel la nuit du 24 au 25 | La Boissaude (1 222 m) : 33 cm le 22, 9 cm le 28 |
