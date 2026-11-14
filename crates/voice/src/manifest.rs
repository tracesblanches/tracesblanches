//! The finite clip set of a pack: fixed phrases, value phrases on their grids, and the
//! names of the pack.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::clips::{self, Clip, DayMonth, Direction, Fixed, TURNAROUND_MINUTES};
use crate::names::{Colour, PisteName, name_clip_id, name_clip_text};
use crate::quantity::{CLOCK_HOURS, Clock, distance_grid, duration_grid};

/// Turn instructions carry their distance up to 2 km; beyond, "Dans X." then the turn.
pub const TURN_MAX_M: u32 = 2000;
/// Distance to the route when off it, up to 2 km; beyond, "plus de deux kilomètres".
pub const AT_MAX_M: u32 = 2000;
/// "Dans X." clips, up to 20 km.
pub const AHEAD_MAX_M: u32 = 20_000;
/// Zone lengths ("Sur X."), 100 m to 20 km.
pub const OVER_MAX_M: u32 = 20_000;
/// Slopes announced, degrees (30..=45).
pub const SLOPES: (u32, u32) = (30, 45);
/// Temperatures, °C.
pub const TEMPERATURES: (i32, i32) = (-35, 30);
/// Wind, km/h, by 10.
pub const WINDS: (u32, u32) = (10, 150);

/// Names of a pack, from its layers (pistes, refuges, places, zones).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackNames {
    /// Site name ("Cervières").
    pub site: String,
    /// Pistes with their colours.
    pub pistes: Vec<PisteName>,
    /// Refuges, shelters, places, protected zones.
    pub places: Vec<String>,
    /// End dates of the seasonal protection periods of the pack's zones.
    pub dates: Vec<DayMonth>,
    /// Number of pistes whose open status is announced (0..=this many open).
    pub status_pistes: u32,
}

/// Kind of a clip, for size accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipKind {
    /// Fixed phrase.
    Fixed,
    /// Value on a grid (distance, duration, time, slope, temperature...).
    Value,
    /// Name of the pack.
    Name,
}

/// Clip id → (kind, text) for one pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Clips by id.
    pub clips: BTreeMap<String, (ClipKind, String)>,
}

impl Manifest {
    fn add(&mut self, kind: ClipKind, c: Clip) {
        // Names that differ only by case or accents share an id: the first text is kept.
        self.clips.entry(c.id).or_insert((kind, c.text));
    }

    /// Whether every clip of `a` is in the manifest with the same text (if not, the
    /// announcement goes to the fallback voice).
    pub fn covers(&self, a: &crate::bank::Announcement) -> bool {
        a.segments.iter().all(|s| {
            self.clips
                .get(&s.clip.id)
                .is_some_and(|(_, t)| *t == s.clip.text)
        })
    }

    /// Number of clips of each kind.
    pub fn count(&self, kind: ClipKind) -> usize {
        self.clips.values().filter(|(k, _)| *k == kind).count()
    }
}

/// The clip set of a pack with these names.
pub fn manifest(names: &PackNames) -> Manifest {
    let mut m = Manifest::default();
    for f in Fixed::ALL {
        m.add(ClipKind::Fixed, f.clip());
    }
    let grid = distance_grid();
    for d in Direction::ALL {
        m.add(ClipKind::Value, clips::turn(d, None));
        for &g in grid.iter().filter(|&&g| g <= TURN_MAX_M) {
            m.add(ClipKind::Value, clips::turn(d, Some(g)));
        }
    }
    for c in Colour::ALL {
        m.add(ClipKind::Value, clips::fork(c));
    }
    for &g in &grid {
        if g <= AHEAD_MAX_M {
            m.add(ClipKind::Value, clips::ahead(g));
        }
        if g <= AT_MAX_M {
            m.add(ClipKind::Value, clips::at(g));
        }
        if (100..=OVER_MAX_M).contains(&g) {
            m.add(ClipKind::Value, clips::over(g));
        }
        if g >= 100 {
            m.add(ClipKind::Value, clips::remain(Some(g)));
        }
    }
    m.add(ClipKind::Value, clips::remain(None));
    m.add(ClipKind::Value, clips::about(None));
    for g in duration_grid() {
        m.add(ClipKind::Value, clips::about(Some(g)));
    }
    for c in Clock::quarters(CLOCK_HOURS.0, CLOCK_HOURS.1) {
        m.add(ClipKind::Value, clips::clock(c));
    }
    for t in TURNAROUND_MINUTES {
        m.add(ClipKind::Value, clips::turnaround_in(t));
    }
    for d in SLOPES.0..=SLOPES.1 {
        m.add(ClipKind::Value, clips::slope(d));
    }
    for t in TEMPERATURES.0..=TEMPERATURES.1 {
        m.add(ClipKind::Value, clips::temperature(t));
    }
    for w in (WINDS.0..=WINDS.1).step_by(10) {
        m.add(ClipKind::Value, clips::wind(w));
    }
    for l in 1..=5 {
        m.add(ClipKind::Value, clips::bera(l));
    }
    for n in 0..=names.status_pistes {
        m.add(ClipKind::Value, clips::pistes_open(n));
    }
    for p in (0..=100).step_by(10) {
        m.add(ClipKind::Value, clips::battery(p));
    }
    for d in &names.dates {
        m.add(ClipKind::Value, clips::until(*d));
    }
    m.add(
        ClipKind::Name,
        Clip {
            id: name_clip_id(&names.site),
            text: name_clip_text(&names.site),
        },
    );
    for p in &names.pistes {
        m.add(
            ClipKind::Name,
            Clip {
                id: p.clip_id(),
                text: p.clip_text(),
            },
        );
    }
    for n in &names.places {
        m.add(
            ClipKind::Name,
            Clip {
                id: name_clip_id(n),
                text: name_clip_text(n),
            },
        );
    }
    m
}
