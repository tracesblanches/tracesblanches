//! Synthetic tracks reaching every reason and every piece of advice (tests of the texts).

use crate::analysis::tests::synthetic;
use crate::field::ZoneFlags;
use crate::profile::Profile;
use crate::rules::{Conditions, Params};

/// One case: a name, a track, the parameters and the conditions of the day.
pub(crate) type Case = (&'static str, Profile, Params, Option<Conditions>);

fn tp() -> Params {
    Params {
        effort: crate::rules::Effort::RaidOnly,
        forest_cover_pct: Some(70.0),
        ..Params::scale()
    }
}

/// Every case.
pub(crate) fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    out.push(("flat", synthetic(&[(2000.0, 0.0)]), tp(), None));
    out.push((
        "gentle",
        synthetic(&[(1000.0, 0.0), (300.0, 6.0), (300.0, -5.0)]),
        tp(),
        None,
    ));
    out.push((
        "climb and descent",
        synthetic(&[(1000.0, 0.0), (300.0, 16.0), (4000.0, 0.0), (400.0, -9.0)]),
        tp(),
        None,
    ));
    for deg in [10.0, 13.0, 17.0, 22.0, 27.0, 32.0] {
        let mut p = synthetic(&[(1500.0, 0.0), (300.0, deg), (300.0, -deg), (500.0, 0.0)]);
        for (i, s) in p.samples.iter_mut().enumerate() {
            // North-facing on the way up, south-facing on the way down.
            s.aspect = Some(if i < 180 { 0.0 } else { 180.0 });
        }
        out.push(("steep", p.clone(), Params::scale(), None));
        out.push(("steep default", p, Params::default(), None));
    }
    // Dense forest descent.
    let mut p = synthetic(&[(2000.0, 0.0), (200.0, -12.0), (500.0, 0.0)]);
    for s in &mut p.samples {
        s.cover = Some(100.0);
        s.min_h = Some(12.0);
    }
    out.push(("forest", p, tp(), None));
    // Side hill: a flat track across a 28° slope, open terrain.
    let mut p = synthetic(&[(500.0, 0.0), (400.0, 0.0), (2600.0, 0.0)]);
    for (i, s) in p.samples.iter_mut().enumerate() {
        if (50..90).contains(&i) {
            s.slope = Some(28.0);
        }
        s.cover = Some(0.0);
    }
    out.push(("sidehill", p, tp(), None));
    // Avalanche exposure.
    let mut p = synthetic(&[(3000.0, 0.0), (300.0, -14.0)]);
    p.inputs.exposure = true;
    for (i, s) in p.samples.iter_mut().enumerate() {
        s.exposure = Some(match i {
            50..=80 => 2,
            100..=140 => 3,
            200..=210 => 3,
            _ => 1,
        });
    }
    out.push(("exposed", p, tp(), None));
    // Exposure not evaluated on steep terrain.
    let mut p = synthetic(&[(1000.0, 0.0), (300.0, 26.0), (1000.0, 0.0)]);
    p.inputs.exposure = false;
    out.push(("exposure unknown", p, tp(), None));
    // Relief unknown.
    let mut p = synthetic(&[(2000.0, 0.0), (300.0, -10.0)]);
    for s in p.samples.iter_mut().skip(20).take(40) {
        s.z = None;
    }
    out.push(("no relief", p.clone(), tp(), None));
    p.inputs.exposure = true;
    for s in &mut p.samples {
        s.exposure = Some(1);
    }
    out.push(("no relief, exposure known", p, tp(), None));
    // Far from an exit, no network, zones.
    let mut p = synthetic(&[(16000.0, 0.0), (500.0, 8.0), (300.0, -18.0)]);
    p.inputs.network = true;
    p.inputs.zones = true;
    for (i, s) in p.samples.iter_mut().enumerate() {
        s.no_network = Some((300..700).contains(&i));
        s.zone = match i {
            100..=150 => Some(ZoneFlags {
                access_forbidden: true,
                names: vec!["Zone A".into()],
                ..ZoneFlags::default()
            }),
            200..=260 => Some(ZoneFlags {
                leave_routes_forbidden: true,
                names: vec!["Zone B".into()],
                ..ZoneFlags::default()
            }),
            800..=900 => Some(ZoneFlags {
                avoid: true,
                names: vec!["Zone C".into()],
                ..ZoneFlags::default()
            }),
            _ => Some(ZoneFlags::default()),
        };
    }
    out.push(("remote", p.clone(), tp(), None));
    out.push(("remote default", p, Params::default(), None));
    // Conditions of the day.
    let p = synthetic(&[(2000.0, 0.0), (300.0, 13.0), (300.0, -13.0)]);
    for c in [
        Conditions {
            fresh_snow_cm: Some(25.0),
            ..Conditions::default()
        },
        Conditions {
            icy: true,
            ..Conditions::default()
        },
        Conditions {
            crust: true,
            ..Conditions::default()
        },
    ] {
        out.push(("conditions", p.clone(), tp(), Some(c)));
    }
    // Approximate track.
    let mut p = synthetic(&[(3000.0, 0.0), (200.0, 9.0)]);
    for s in &mut p.samples {
        s.seg_len = 300.0;
    }
    out.push(("coarse", p, tp(), None));
    // Raid and long day.
    out.push((
        "raid",
        synthetic(&[(35000.0, 0.0), (1000.0, 12.0)]),
        Params::scale(),
        None,
    ));
    out.push((
        "long day",
        synthetic(&[
            (9000.0, 0.0),
            (2500.0, 14.0),
            (8000.0, 0.0),
            (1000.0, -10.0),
        ]),
        Params::scale(),
        None,
    ));
    // The hardest passage far from the start.
    out.push((
        "late",
        synthetic(&[(6000.0, 0.0), (200.0, -21.0), (200.0, 0.0)]),
        tp(),
        None,
    ));
    out
}
