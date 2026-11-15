# skimap-plan

**Tout se décide le matin, sans gants ; sur les pistes, le téléphone se tait tant que tout va bien.** Le matin : briefing, créneau, fartage, boucles proposées, équipement ; on valide une boucle. Ensuite : départ, pauses, refuge et arrivée reconnus seuls, une seule phrase quand quelque chose change, bascule seule sur un plan B calculé le matin, refusable d'un geste. Tickets : J1 (#110), J2 (#111), J3 (#112), J4 (#113), Q9 (#109), E1 (#125). Jamais « sans danger », jamais « sûr » (testé sur chaque phrase et chaque bilan).

Côté téléphone : aucune pile HTTP (`cargo tree -p skimap-plan -e normal`), compile pour `aarch64-linux-android` (`cargo ndk -t arm64-v8a --platform 34 build -p skimap-plan`).

## Le matin (poste ou téléphone) : `plan::morning`, `plan::build`

| Étape | Source |
|---|---|
| briefing du jour, qualité de neige, fartage, meilleur départ | A1, A3, A2, A9 (`skimap-assist`) au point météo le plus proche du départ |
| boucles pour le temps, la technique, le niveau, l'heure de retour | A5 sur le graphe C1, temps C5 du jour |
| équipement de chaque boucle | A6 (fart de retenue peu fiable : A2) |
| synthèse sur la boucle choisie | A7 |

`plan::build` fait du choix un `DayPlan` (JSON, `DayPlan::from_json` sur le téléphone) : jambes de l'itinéraire sur le graphe, points clés avec heure de passage au rythme du modèle (départ, carrefours dits ou évidents, refuges à moins de 150 m, zones sans réseau P12, couloirs d'avalanche connus P20, arrivée), limite de retour (C7 et heure demandée), demi-tour au plus tard (A8), annonces prévues avec leurs clips et ceux qui manquent au pack (VO2, VO3), plans B, équipement, raisons, météo du jour. `plan::track` en tire la **table à plat** (`.trk`) que le téléphone lit en place à chaque position.

## Plans B (`fallback`, J2)

- **Sorties** : à chaque carrefour utile (de 1 km après le départ à 1 km avant l'arrivée, 1 km d'écart), le plus rapide vers l'arrivée s'il fait gagner 10 min. Brouillard ou vent qui se lève : la sortie qui gagne le plus. Retard sur la limite : la plus longue qui finit à temps (boucle écourtée).
- **Contournements** : pour chaque voie OSM de la boucle, du dernier carrefour avant elle au premier après, sans elle ; trop long (plus de 25 % du temps, 20 min au moins) ou hors limite : directement vers l'arrivée ; sinon « aucun autre passage connu ». Les voies au même contournement le partagent.
- Pendant la sortie, sans plan B qui convienne : itinéraire calculé sur place (graphe du pack, voies fermées interdites).

## Suivi (`tracker`, `session`, `speech`, `gps`)

Par position, sans allocation : projection sur la fenêtre de l'itinéraire devant le curseur (premier passage, pas le plus proche : épingles et allers-retours), curseur qui n'avance que de ce qu'un skieur peut faire, écart (C2 : 40 m pendant 15 s), points de décision franchis, mode GPS, parole. Une fois par minute : alertes A8 et limite de retour. `Tracker::push_batch` prend un lot de positions (jusqu'à 60 s, rapport 09).

| Décision | Règle |
|---|---|
| départ | près du départ et de l'itinéraire, hors voiture, 40 m en 40 s (et 30 m le long de l'itinéraire), daté de la fin de l'attente |
| pause | 90 s sous 0,3 m/s après « immobile » d'Android, sous 0,15 m/s avec les seules positions ; progression le long de l'itinéraire comprise |
| refuge | pause à moins de 50 m d'un refuge du plan pendant 5 min |
| reprise | mouvement significatif (GPS à 1 s), 40 m hors de la pause |
| fin | pause près de l'arrivée, ou voiture (abandon loin de l'arrivée) ; bilan |
| GPS | poche si D > 300 m (5 s, lot ≤ 60 s), approche si D ≤ 300 m ou 45 s (1 s), écart au-delà de 25 m lissés (1 s), pause et refuge GNSS coupé ; 30 s au moins par mode |

Parole (J4) : sécurité > direction > horaire > confort ; regroupement 8 s ; 12 groupes par heure au plus hors sécurité (confort : la moitié ; directions exemptées par défaut, réglable) ; pendant une pause, seule la sécurité parle ; **une alerte de sécurité n'est jamais perdue** (propriété proptest, 512 cas : dite une fois, au tick de son offre, dans l'ordre). Rien n'est dit à un carrefour évident (toutes les branches à 45° au moins, tout droit ou même piste qui tourne).

## Banc Q9 (`scenario`, `tests/scenarios.rs`)

23 sorties sur les deux sites, en boucle fermée avec horloge simulée : le skieur suit le modèle C5, le GPS a le bruit de C2 (σ 10 ou 15 m, forêt), le client applique les modes GPS et livre par lots. Normal, GPS dégradé, arrivée en voiture, écart et retour, écart puis recalcul, piste fermée (plan B, refus d'un geste, refus en continuant), retard (heure de retour, nuit, départ tardif, plus le temps), brouillard, vent, pause au refuge, pause courte, abandon en voiture ou à pied, trou GPS, débit plafonné, modes GPS. Chaque course est rejouée à l'identique (déterminisme).

```sh
cargo run --release -p skimap-plan --example day_plan      # plans des deux sites → data/out/<site>/plan/
cargo run --release -p skimap-plan --example track_bench   # coût du plan et du suivi par position
cargo test --release -p skimap-plan                        # dont tests/scenarios.rs, tests/alloc.rs
```

Mesures (15/11/2026, poste, release) : plan 0,06 s à Cervières, 0,13 à 0,27 s à Chapelle-des-Bois (matin A1–A6 : 0,02 / 0,18 s) ; suivi d'une sortie de 3 h : 6,3 ms à Cervières (1,3 µs par position, 0,28 µs médiane hors événement, p99 1,1 µs), 1,6 ms à Chapelle-des-Bois ; 0 allocation sur 4 449 et 3 615 positions sans événement.

## Limites

- Phrases du plan (`fix.plan-*`) absentes de la banque VO2 : voix de secours tant qu'elles n'y sont pas.
- Traces synthétiques seulement ; les seuils (pause, départ, GPS) restent à régler sur le terrain (T5) et sur le Pixel.
- Les routes calculées pendant la sortie n'annoncent ni zones sans réseau ni couloirs d'avalanche (couches non lues par le suivi).
- Les tests lisent `data/out/` : à brancher sur `fixtures/` (Q1).
