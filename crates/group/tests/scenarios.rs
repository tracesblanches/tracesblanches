//! Outings of a group replayed with a simulated clock (report 10, §11.4): side by side for
//! an hour, a separation of 600 m, a stop of 15 min, a departure from the route, a relay.
//!
//! The radio: every 10 s (the screen-off scan window), each phone hears each frame of a
//! phone within [`RANGE_M`] with probability [`P_HEARD`] (the body masks the link), its own
//! frame and one relay in turn. Skiers walk along a meridian; their progress along the
//! common plan is the distance walked.

use skimap_group::{AlertKind, Config, Group, OwnFix, Phase, Session, Source, plan_fingerprint};

const T0: i64 = 1_768_036_800_000; // 2026-01-10 09:20 UTC
const STEP_MS: i64 = 10_000;
const RANGE_M: f64 = 30.0;
const P_HEARD: f64 = 0.4;
const M_PER_DEG: f64 = 111_195.0;

/// Deterministic pseudo-random numbers (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Skier {
    session: Session,
    /// Metres along the plan.
    at_m: f64,
    phase: Phase,
    off_route: bool,
    alerts: Vec<(i64, skimap_group::Alert)>,
}

struct World {
    skiers: Vec<Skier>,
    now: i64,
    rng: Rng,
}

fn group(n: usize) -> Group {
    let names = ["Pierre", "Léa", "Jo"][..n]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    Group::create([42u8; 32], names, plan_fingerprint(b"plan of the day"), 0).unwrap()
}

impl World {
    fn new(n: usize, seed: u64) -> Self {
        let g = group(n);
        let skiers = (0..n)
            .map(|i| Skier {
                session: Session::new(g.clone(), i as u8, Config::default()).unwrap(),
                at_m: 0.0,
                phase: Phase::Moving,
                off_route: false,
                alerts: Vec::new(),
            })
            .collect();
        Self {
            skiers,
            now: T0,
            rng: Rng(seed),
        }
    }

    fn lat(at_m: f64) -> f64 {
        44.87 + at_m / M_PER_DEG
    }

    /// One step of 10 s: everyone moves by `speed[i]` m/s, sends, hears, checks alerts.
    fn step(&mut self, speed: &[f64]) {
        self.now += STEP_MS;
        let now = self.now;
        for (s, v) in self.skiers.iter_mut().zip(speed) {
            s.at_m += v * STEP_MS as f64 / 1000.0;
            s.session.own(OwnFix {
                t_ms: now,
                lat: Self::lat(s.at_m),
                lon: 6.73,
                phase: s.phase,
                off_route: s.off_route,
                progress_m: Some(s.at_m),
                battery_low: false,
            });
        }
        let mut air: Vec<(usize, [u8; 21])> = Vec::new();
        for (i, s) in self.skiers.iter_mut().enumerate() {
            air.extend(s.session.own_frame(now).map(|f| (i, f)));
            air.extend(s.session.relay_frame(now).map(|f| (i, f)));
        }
        let at: Vec<f64> = self.skiers.iter().map(|s| s.at_m).collect();
        for (j, s) in self.skiers.iter_mut().enumerate() {
            for (i, f) in &air {
                if *i != j && (at[*i] - at[j]).abs() <= RANGE_M && self.rng.next() < P_HEARD {
                    s.session.receive(now, f).unwrap();
                }
            }
            for a in s.session.alerts(now) {
                s.alerts.push((now, a));
            }
        }
    }

    fn run(&mut self, seconds: i64, speed: &[f64]) {
        for _ in 0..seconds * 1000 / STEP_MS {
            self.step(speed);
        }
    }
}

#[test]
fn side_by_side_for_an_hour_raises_nothing() {
    for seed in 1..=20 {
        let mut w = World::new(3, seed);
        // Everyone at 2 m/s; gaps of up to 24 m that come and go.
        for k in 0..360 {
            let wobble = if k % 30 < 15 { 0.08 } else { -0.08 };
            w.step(&[2.0, 2.0 + wobble, 2.0 - wobble]);
        }
        let alerts: usize = w.skiers.iter().map(|s| s.alerts.len()).sum();
        assert_eq!(alerts, 0, "seed {seed}");
        // Everyone heard everyone recently.
        for s in &w.skiers {
            for m in s.session.members(w.now) {
                assert!(m.age_s.unwrap() < 120, "seed {seed}: {m:?}");
            }
        }
    }
}

#[test]
fn a_separation_of_600_m_raises_one_straggler_alert() {
    let mut w = World::new(2, 7);
    // Together for 10 min, then Léa is 0.5 m/s slower: 300 m in 10 min, 600 m in 20 min.
    w.run(600, &[2.0, 2.0]);
    let split = w.now;
    w.run(1_500, &[2.0, 1.5]);
    let a = &w.skiers[0].alerts;
    assert_eq!(a.len(), 1, "{a:?}");
    let (t, alert) = &a[0];
    assert_eq!(alert.kind, AlertKind::Straggler);
    assert_eq!(alert.member, 1);
    // Out of range after 60 s (30 m), then 5 min of silence: raised ≈ 6 min after the split,
    // when the last position heard is > 500 m behind? Not yet: the last position heard is
    // ≈ 30 m behind; the alert needs it 500 m behind along the plan: Pierre must be 500 m
    // beyond it, ≈ 4 min 10 s at 2 m/s after the loss, so the 5 min of silence decide.
    let delay_s = (t - split) / 1000;
    assert!(
        (300..=420).contains(&delay_s),
        "raised {delay_s} s after the split"
    );
    assert!(alert.age_s > 300 && alert.behind_m.unwrap() > 500.0);
    assert!(
        alert
            .text
            .starts_with("Léa : pas de nouvelles depuis 5 min, dernière position ")
    );
    assert!(alert.text.contains("derrière à 10 h 3"), "{}", alert.text);
    // Léa's phone: Pierre is ahead, not behind: nothing.
    assert!(w.skiers[1].alerts.is_empty(), "{:?}", w.skiers[1].alerts);
    // Back together: the alert is re-armed, not repeated while together.
    w.skiers[1].at_m = w.skiers[0].at_m;
    w.run(600, &[2.0, 2.0]);
    assert_eq!(w.skiers[0].alerts.len(), 1);
}

#[test]
fn a_stop_of_15_min_away_from_a_refuge_is_told_once() {
    let mut w = World::new(2, 11);
    w.run(300, &[2.0, 2.0]);
    w.skiers[1].phase = Phase::Paused;
    let stop = w.now;
    // Pierre comes and goes around Léa, within range.
    for k in 0..90 {
        let v = if k % 4 < 2 { 1.0 } else { -1.0 };
        w.step(&[v, 0.0]);
    }
    let a = &w.skiers[0].alerts;
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].1.kind, AlertKind::LongStop);
    let delay_s = (a[0].0 - stop) / 1000;
    // 10 min of pause, then the next frame heard.
    assert!((600..=700).contains(&delay_s), "{delay_s}");
    assert!(
        a[0].1
            .text
            .starts_with("Léa : arrêt prolongé hors d'un refuge")
    );
    // At a refuge, a long pause is no alert.
    let mut w = World::new(2, 12);
    w.skiers[1].phase = Phase::Refuge;
    w.run(1_800, &[0.0, 0.0]);
    assert!(w.skiers[0].alerts.is_empty());
}

#[test]
fn leaving_the_common_route_is_told_each_time() {
    let mut w = World::new(2, 5);
    w.run(120, &[1.0, 1.0]);
    w.skiers[1].off_route = true;
    w.run(120, &[1.0, 1.0]);
    w.skiers[1].off_route = false;
    w.run(120, &[1.0, 1.0]);
    w.skiers[1].off_route = true;
    w.run(120, &[1.0, 1.0]);
    let kinds: Vec<AlertKind> = w.skiers[0].alerts.iter().map(|(_, a)| a.kind).collect();
    assert_eq!(kinds, vec![AlertKind::OffRoute, AlertKind::OffRoute]);
}

#[test]
fn positions_travel_hop_by_hop() {
    let mut w = World::new(3, 3);
    // Pierre ahead, out of range of Léa; Jo goes between them.
    w.skiers[0].at_m = 1_000.0;
    w.skiers[2].at_m = 0.0;
    w.run(120, &[0.0, 0.0, 0.0]);
    let heard_at = w.now;
    // Jo walks to Pierre at 4 m/s (250 s), Léa stays out of range of both.
    w.run(260, &[0.0, 0.0, 4.0]);
    w.run(60, &[0.0, 0.0, 0.0]);
    let pierre = w.skiers[0].session.members(w.now);
    let lea = pierre.iter().find(|m| m.member == 1).unwrap();
    assert!(matches!(lea.source, Some(Source::Relayed(1))), "{lea:?}");
    // Léa's position is the one Jo heard before leaving: its age is told as it is.
    let (_, _, t) = lea.position.unwrap();
    assert!(t * 1000 <= heard_at && lea.age_s.unwrap() >= 320);
    assert!((lea.distance_m.unwrap() - 1_000.0).abs() < 5.0);
}
