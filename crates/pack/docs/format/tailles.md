# Format du pack : tailles et contrôles

Partie de [Format du pack](../format.md).

## Tailles des packs (15/11/2026, 10 h 36, pente et exposition en vecteur)

Packs construits avec `personal_sources` (usage personnel de l'auteur) et sans les tuiles IGN de comparaison ; entre parenthèses, les mêmes sorties construites à 9 h 39 avec la pente et l'exposition en rasters PNG :

| Partie | Jeu de test alpin | Jeu de test jurassien |
|---|---|---|
| Fond Plan IGN v2 (z0-16, style, sprite, glyphes) | 13,13 Mo | 12,95 Mo |
| Fond OSM Protomaps (z0-15, style, sprite, glyphes) | 4,62 Mo | 5,58 Mo |
| Couches `layers/` | 23,73 Mo (18,95) | 18,10 Mo (16,80) |
| dont rasters hors piste de core (gzip) : altitude / pente | 8,53 / 5,73 Mo | 5,67 / 4,25 Mo |
| dont pente et exposition en polygones (MVT) | 4,74 Mo | 1,15 Mo |
| Rasters en tuiles (neige z9-13 ; avant : + pente et exposition z10-15) | 0,22 Mo (4,35) | 0,10 Mo (1,38) |
| Voix (clips Opus) | 4,07 Mo | 5,07 Mo |
| **Total** | **45,79 Mo** (45,14), 986 fichiers | **41,82 Mo** (41,79), 1 225 fichiers |
| Couches à usage personnel incluses (zones avec Biodiv'sports, exposition avec la CLPA et ce qui en dérive, état, plan, météo, assistant) | 18,79 Mo | 13,19 Mo |
| Option tuiles IGN des pentes (z11-15, RGBA) | +22,63 Mo | +13,17 Mo |

L'altitude 5 m du routeur hors piste pesait un tiers du pack : core écrit maintenant ses rasters SKR1 en différences le long des lignes (octets faibles puis forts), et le gzip de l'altitude passe de 17,1 à 8,6 Mo sur le jeu de test alpin, de 10,5 à 5,7 Mo sur le jeu de test jurassien (zlib 6). Le format SKR1 appartient à core (`crates/core/docs/horspiste.md`).

Temps de construction depuis les caches : 2,2 s (jeu de test alpin) et 1,8 s (jeu de test jurassien) avec les rasters PNG ; 4,0 s et 3,2 s avec les polygones (poste chargé par d'autres compilations : jusqu'à 6,3 s et 4,1 s), dont 1,5 s et 0,65 s pour la pente, 0,7 s et 1,0 s pour l'exposition.

## Contrôles

- `tests/synthetic_pack.rs` : pack construit sur des sorties synthétiques sans réseau, relu, exporté ; sources à usage personnel écartées sans l'option.
- `tests/real_packs.rs` (données réelles, `--ignored`) : `Pack::verify` sans écart ; chaque couche GeoJSON garde toutes les entités de sa source ; jointure zones-règles identique à la liste de la crate zones (`zones.meta.json` : 23 paires sur le jeu de test alpin, 33 sur le jeu de test jurassien) ; tous les pixels contrôlés (un sur neuf) des tuiles de neige au zoom maximal égaux à la classe de la cellule GeoTIFF sous leur centre (0 écart sur 0,27 M par couche) ; polygones de pente et d'exposition redessinés au zoom 15 comparés aux GeoTIFF (voir plus haut) ; champs `bivouac` et `access` des zones sur les règles réelles ; état sur chaque voie nommée par un itinéraire de ski ; pack public sans source personnelle.
