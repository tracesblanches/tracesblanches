//! What the phone reads on every fix, computed on the workstation when the plan is made
//! (E1 #125): for the loop and each plan B, the route geometry in a local plane, a grid
//! index of its segments, the decision points (trigger, point, end) sorted along it, and
//! the model time profile. Stored flat (little endian, 4-byte aligned) and read in place
//! by [`TrackView`] / [`RouteView`]: no parse, no copy, no allocation.
//!
//! A plan B is the loop up to its divergence, its own legs, then (a detour that comes back)
//! the loop again to the finish: its block keeps only its own part and refers to the
//! loop's block for the rest (X13 #147): the vertices of the loop up to the divergence
//! (`head`), those from where it comes back (`tail_from`, with the difference of the
//! distances along), and the same for the time profile (the loop keeps a vertex and a
//! profile station at each divergence and return). Its grid indexes its own segments; the
//! shared ones are found in the loop's grid.
//!
//! # File (`plan_<…>.trk`)
//!
//! ```text
//! magic "SKTRK002", origin lon f64, lat f64, routes u32, 0 u32, offset u32 per route
//! route: id i32 (0 loop, i+1 plan B i, -1 computed on the phone), s_div f32, scale f32,
//!        length f32, points u32, decisions u32, profile u32, nx u32, ny u32, cell f32,
//!        x0 f32, y0 f32, index u32,
//!        head u32, tail_from u32, tail u32, tail_ds f32,
//!        prof_head u32, prof_tail_from u32, prof_tail u32, d_shift f32, out_shift f32,
//!        back_shift f32
//!        x f32[points], y f32[points], s f32[points]
//!        decision[decisions]: s_trigger f32, s_point f32, s_until f32, ann u32, src u32,
//!                             priority u8, kind u8, 0 u16
//!        profile: dist f32[profile], out f32[profile], back f32[profile]
//!        grid: start u32[nx·ny + 1], segment u32[index]
//! ```
//!
//! `points` and `profile` count the route's own vertices and stations. The route's vertex
//! `i` is the loop's vertex `i` below `head`, its own vertex `i − head` up to `head +
//! points`, then the loop's vertex `tail_from + i − head − points` (its distance along plus
//! `tail_ds`), `tail` of them; the profile likewise (`prof_*`, distance, out and back times
//! plus the shifts). Only a plan B refers to the loop, the first route of the file; the
//! loop and a route computed on the phone have `head`, `tail` and `prof_*` at 0. A
//! segment of the grid is the global index `k` of its first vertex.
//!
//! Coordinates are metres east and north of the origin (the plan's start) on the local
//! plane of `skimap_core::graph::LocalPlane`; `s` is the distance along the route of the
//! [`crate::course::Course`] (haversine), kept at every vertex left by the simplification
//! (Douglas–Peucker, [`SIMPLIFY_M`]). `scale` turns `s` into the distance of the time
//! profile (A8 works on the profile). A decision's announcement is `ann` in the list
//! `src` of the plan (0: the loop's `announcements`, i+1: plan B i's).

use skimap_assist::suggest::Profile;
use skimap_common::geom::LonLat;
use skimap_core::graph::LocalPlane;

use crate::course::Course;
use crate::error::{Error, Result};
use crate::plan::{PlannedAnnouncement, Trigger};
use crate::speech::Priority;

/// Simplification tolerance of the route geometry, metres.
pub const SIMPLIFY_M: f64 = 2.0;
/// Grid cell of the segment index, metres.
pub const CELL_M: f64 = 200.0;
/// Spacing of the time profile stations kept, metres (A8 works on minutes).
pub const PROFILE_M: f64 = 50.0;
const MAGIC: &[u8; 8] = b"SKTRK002";
const ROUTE_HEADER: usize = 23 * 4;
const DECISION: usize = 24;

/// Kind of decision point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DecisionKind {
    /// A junction where a turn is said.
    Turn = 0,
    /// A zone without network ahead.
    NoNetwork = 1,
    /// End of a zone without network.
    NoNetworkEnd = 2,
    /// A historic avalanche path ahead.
    Avalanche = 3,
    /// A protected zone ahead (X11).
    ProtectedZone = 4,
    /// Anything else.
    Other = 255,
}

impl DecisionKind {
    fn of(kind: &str) -> Self {
        match kind {
            "turn" => DecisionKind::Turn,
            "no_network" => DecisionKind::NoNetwork,
            "no_network_end" => DecisionKind::NoNetworkEnd,
            "avalanche_path" => DecisionKind::Avalanche,
            "protected_zone" => DecisionKind::ProtectedZone,
            _ => DecisionKind::Other,
        }
    }

    fn from_u8(b: u8) -> Self {
        match b {
            0 => DecisionKind::Turn,
            1 => DecisionKind::NoNetwork,
            2 => DecisionKind::NoNetworkEnd,
            3 => DecisionKind::Avalanche,
            4 => DecisionKind::ProtectedZone,
            _ => DecisionKind::Other,
        }
    }
}

fn priority_u8(p: Priority) -> u8 {
    match p {
        Priority::Safety => 0,
        Priority::Direction => 1,
        Priority::Schedule => 2,
        Priority::Comfort => 3,
    }
}

fn priority_of(b: u8) -> Priority {
    match b {
        0 => Priority::Safety,
        1 => Priority::Direction,
        2 => Priority::Schedule,
        _ => Priority::Comfort,
    }
}

/// A decision point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    /// Said when the skier reaches this distance, metres.
    pub s_trigger: f32,
    /// The point itself (junction, zone entry), metres.
    pub s_point: f32,
    /// Not said after this distance, metres.
    pub s_until: f32,
    /// Announcement index in the list `src`.
    pub ann: u32,
    /// List of the announcement: 0 the loop, i+1 plan B i, `u32::MAX` the route's own.
    pub src: u32,
    /// Priority.
    pub priority: Priority,
    /// Kind.
    pub kind: DecisionKind,
}

/// What a route takes from the loop (a plan B): see the module.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shared {
    /// Vertices of the loop at the head (up to the divergence, included).
    pub head: u32,
    /// First vertex of the loop at the tail (where a detour comes back).
    pub tail_from: u32,
    /// Vertices of the loop at the tail (to its end).
    pub tail: u32,
    /// Added to the loop's distances along at the tail, metres.
    pub tail_ds: f32,
    /// Profile stations of the loop at the head.
    pub prof_head: u32,
    /// First profile station of the loop at the tail.
    pub prof_tail_from: u32,
    /// Profile stations of the loop at the tail.
    pub prof_tail: u32,
    /// Added to the loop's profile at the tail: distance, time out, time back.
    pub prof_shift: [f32; 3],
}

/// A route of the table, owned (built on the workstation, or on the phone for a route
/// computed during the outing).
#[derive(Debug, Clone, PartialEq)]
pub struct RouteTable {
    /// 0 the loop, i+1 plan B i, -1 computed on the phone.
    pub id: i32,
    /// Where it leaves the loop, metres.
    pub s_div: f32,
    /// Profile distance per route distance.
    pub scale: f32,
    /// Length, metres.
    pub length: f32,
    /// What it takes from the loop.
    pub shared: Shared,
    x: Vec<f32>,
    y: Vec<f32>,
    s: Vec<f32>,
    decisions: Vec<Decision>,
    prof: [Vec<f32>; 3],
    grid: (u32, u32, f32, f32, f32),
    start: Vec<u32>,
    segs: Vec<u32>,
    /// Vertices of the course's line kept, and profile stations kept, in order (what a plan
    /// B built on this route refers to; not written).
    kept: Vec<usize>,
    kept_prof: Vec<usize>,
}

/// Douglas–Peucker on plane points, keeping the first, the last and `forced`: indices
/// kept, in order.
fn simplify(xy: &[[f64; 2]], tol: f64, forced: &[usize]) -> Vec<usize> {
    let n = xy.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    for &f in forced {
        if f < n {
            keep[f] = true;
        }
    }
    let anchors: Vec<usize> = (0..n).filter(|&i| keep[i]).collect();
    let mut stack: Vec<(usize, usize)> = anchors.windows(2).map(|w| (w[0], w[1])).collect();
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (xy[a], xy[b]);
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let l2 = dx * dx + dy * dy;
        let mut far = (0.0, a);
        for (i, p) in xy.iter().enumerate().take(b).skip(a + 1) {
            let t = if l2 > 0.0 {
                (((p[0] - pa[0]) * dx + (p[1] - pa[1]) * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = (p[0] - pa[0] - t * dx).hypot(p[1] - pa[1] - t * dy);
            if d > far.0 {
                far = (d, i);
            }
        }
        if far.0 > tol {
            keep[far.1] = true;
            stack.push((a, far.1));
            stack.push((far.1, b));
        }
    }
    (0..n).filter(|&i| keep[i]).collect()
}

/// Decision points of the announcements triggered at a distance, sorted.
fn decisions_of(anns: &[(u32, u32, &PlannedAnnouncement)]) -> Vec<Decision> {
    let mut decisions: Vec<Decision> = anns
        .iter()
        .filter_map(|(src, ann, a)| match a.trigger {
            Trigger::At { s, until } => {
                let point = match a.event {
                    Some(skimap_voice::Event::Turn { metres, .. }) => s + metres,
                    Some(skimap_voice::Event::NoNetworkAhead { metres, .. }) => s + metres,
                    _ if a.kind == "avalanche_path" => s + crate::plan::AVALANCHE_LEAD_M,
                    _ => s,
                };
                Some(Decision {
                    s_trigger: s as f32,
                    s_point: point.min(until) as f32,
                    s_until: until as f32,
                    ann: *ann,
                    src: *src,
                    priority: a.priority,
                    kind: DecisionKind::of(&a.kind),
                })
            }
            _ => None,
        })
        .collect();
    decisions.sort_by(|a, b| a.s_trigger.total_cmp(&b.s_trigger));
    decisions
}

/// Profile stations kept from `from` (excluded) to `to` (included when `last`): one every
/// [`PROFILE_M`] at least after the previous one kept, plus `forced`.
fn sample_profile(p: &Profile, from: usize, to: usize, last: bool, forced: &[usize]) -> Vec<usize> {
    let mut keep = Vec::new();
    let mut prev = from;
    for i in from + 1..=to {
        if (i == to && last) || forced.contains(&i) || p.dist_m[i] - p.dist_m[prev] >= PROFILE_M {
            if i == to && !last {
                break;
            }
            keep.push(i);
            prev = i;
        }
    }
    keep
}

/// Grid of segments (global index, ends), by bounding box with one cell of margin.
#[allow(clippy::type_complexity)]
fn grid_of(
    segs: &[(u32, [f32; 2], [f32; 2])],
    pts: &[[f32; 2]],
) -> ((u32, u32, f32, f32, f32), Vec<u32>, Vec<u32>) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts.iter().chain(segs.iter().flat_map(|(_, a, b)| [a, b])) {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let cell = CELL_M as f32;
    x0 -= cell;
    y0 -= cell;
    let nx = (((x1 + cell - x0) / cell).ceil() as u32).max(1);
    let ny = (((y1 + cell - y0) / cell).ceil() as u32).max(1);
    let mut cells: Vec<Vec<u32>> = vec![Vec::new(); (nx * ny) as usize];
    for (k, a, b) in segs {
        let (ax, bx) = (a[0].min(b[0]), a[0].max(b[0]));
        let (ay, by) = (a[1].min(b[1]), a[1].max(b[1]));
        let cx = |v: f32| (((v - x0) / cell) as u32).min(nx - 1);
        let cy = |v: f32| (((v - y0) / cell) as u32).min(ny - 1);
        for j in cy(ay)..=cy(by) {
            for i in cx(ax)..=cx(bx) {
                cells[(j * nx + i) as usize].push(*k);
            }
        }
    }
    let mut start = Vec::with_capacity(cells.len() + 1);
    let mut out = Vec::new();
    for c in &cells {
        start.push(out.len() as u32);
        out.extend_from_slice(c);
    }
    start.push(out.len() as u32);
    ((nx, ny, cell, x0, y0), start, out)
}

fn scale_of(course: &Course) -> f32 {
    let line = course.line();
    if line.length_m() > 0.0 {
        (course.profile().length_m() / line.length_m()) as f32
    } else {
        1.0
    }
}

impl RouteTable {
    /// Table of a course with its planned announcements (`src`: their list in the plan).
    pub fn new(
        id: i32,
        s_div: f64,
        course: &Course,
        plane: &LocalPlane,
        anns: &[(u32, u32, &PlannedAnnouncement)],
    ) -> Self {
        let line = course.line();
        let xy: Vec<[f64; 2]> = line.points().iter().map(|p| plane.xy(*p)).collect();
        let kept = simplify(&xy, SIMPLIFY_M, &[]);
        let cum = line.cumulative();
        let x: Vec<f32> = kept.iter().map(|&i| xy[i][0] as f32).collect();
        let y: Vec<f32> = kept.iter().map(|&i| xy[i][1] as f32).collect();
        let s: Vec<f32> = kept.iter().map(|&i| cum[i] as f32).collect();
        // Time profile: the first station, one every PROFILE_M at least, the last.
        let p = course.profile();
        let mut kept_prof = vec![0];
        kept_prof.extend(sample_profile(p, 0, p.dist_m.len() - 1, true, &[]));
        let prof = [
            kept_prof.iter().map(|&i| p.dist_m[i] as f32).collect(),
            kept_prof.iter().map(|&i| p.out_s[i] as f32).collect(),
            kept_prof.iter().map(|&i| p.back_s[i] as f32).collect(),
        ];
        let pts: Vec<[f32; 2]> = x.iter().zip(&y).map(|(a, b)| [*a, *b]).collect();
        let segs: Vec<(u32, [f32; 2], [f32; 2])> = pts
            .windows(2)
            .enumerate()
            .map(|(k, w)| (k as u32, w[0], w[1]))
            .collect();
        let (grid, start, segs) = grid_of(&segs, &pts);
        Self {
            id,
            s_div: s_div as f32,
            scale: scale_of(course),
            length: line.length_m() as f32,
            shared: Shared::default(),
            x,
            y,
            s,
            decisions: decisions_of(anns),
            prof,
            grid,
            start,
            segs,
            kept,
            kept_prof,
        }
    }

    /// Table of a plan B on the loop's (`base`, [`RouteTable::new`] of `base_course`): its
    /// first `head_legs` legs and its last `tail_legs` legs are the loop's, read from the
    /// loop's block (its vertices and profile stations up to the divergence and from the
    /// return; the plan B keeps the divergence and the return among its own when the loop's
    /// simplification dropped them). `None` when the legs do not match the loop's (the
    /// caller writes the route whole).
    #[allow(clippy::too_many_arguments)]
    pub fn on_loop(
        base: &RouteTable,
        base_course: &Course,
        id: i32,
        s_div: f64,
        course: &Course,
        plane: &LocalPlane,
        anns: &[(u32, u32, &PlannedAnnouncement)],
        head_legs: usize,
        tail_legs: usize,
    ) -> Option<Self> {
        let (bl, cb) = (base_course, course);
        let (nl, nb) = (bl.legs().len(), cb.legs().len());
        if head_legs > nl.min(nb)
            || tail_legs > nl.min(nb)
            || head_legs + tail_legs >= nb
            || bl.legs()[..head_legs] != cb.legs()[..head_legs]
            || bl.legs()[nl - tail_legs..] != cb.legs()[nb - tail_legs..]
            || base.kept.is_empty()
        {
            return None;
        }
        let (lp, bp) = (bl.line().points(), cb.line().points());
        let (lc, bc) = (bl.line().cumulative(), cb.line().cumulative());
        let (lps, bps) = (bl.profile_leg_stations(), cb.profile_leg_stations());
        let (lpr, bpr) = (bl.profile(), cb.profile());
        // Divergence: the same vertex and profile station on both (the same legs before).
        let jd = bl.vertex_at(bl.leg_bounds()[head_legs].0)?;
        if cb.vertex_at(cb.leg_bounds()[head_legs].0)? != jd || lp[jd] != bp[jd] {
            return None;
        }
        let ipd = lps[head_legs];
        if bps[head_legs] != ipd || bpr.dist_m[ipd] != lpr.dist_m[ipd] {
            return None;
        }
        // Loop's vertices and stations up to the divergence (at least the start).
        let head = base.kept.partition_point(|&k| k <= jd).max(1);
        let prof_head = base.kept_prof.partition_point(|&k| k <= ipd).max(1);
        let mut sh = Shared {
            head: head as u32,
            prof_head: prof_head as u32,
            ..Shared::default()
        };
        // Own part: from the loop's last vertex at the head to the loop's first vertex at
        // the tail (or the end), the divergence and the return kept.
        let a = base.kept[head - 1];
        let pa = base.kept_prof[prof_head - 1];
        let (mut end, mut pend) = (bp.len() - 1, bpr.dist_m.len() - 1);
        let mut forced = vec![jd - a];
        let mut forced_p = vec![ipd];
        if tail_legs > 0 {
            let (rl, rb) = (nl - tail_legs, nb - tail_legs);
            let jr = bl.vertex_at(bl.leg_bounds()[rl].0)?;
            let jrb = cb.vertex_at(cb.leg_bounds()[rb].0)?;
            let (ipr, iprb) = (lps[rl], bps[rb]);
            if lp[jr] != bp[jrb]
                || lp.len() - jr != bp.len() - jrb
                || jrb <= jd
                || lpr.dist_m.len() - ipr != bpr.dist_m.len() - iprb
                || iprb <= ipd
            {
                return None;
            }
            // The loop's first vertex and station at or after the return.
            let kr = base.kept.partition_point(|&k| k < jr);
            let pr = base.kept_prof.partition_point(|&k| k < ipr);
            if kr >= base.kept.len() || pr >= base.kept_prof.len() {
                return None;
            }
            let (jt, it) = (base.kept[kr], base.kept_prof[pr]);
            end = jrb + (jt - jr);
            pend = iprb + (it - ipr);
            sh.tail_from = kr as u32;
            sh.tail = (base.kept.len() - kr) as u32;
            sh.tail_ds = (bc[end] - lc[jt]) as f32;
            sh.prof_tail_from = pr as u32;
            sh.prof_tail = (base.kept_prof.len() - pr) as u32;
            sh.prof_shift = [
                (bpr.dist_m[pend] - lpr.dist_m[it]) as f32,
                (bpr.out_s[pend] - lpr.out_s[it]) as f32,
                (bpr.back_s[pend] - lpr.back_s[it]) as f32,
            ];
            forced.push(jrb - a);
            forced_p.push(iprb);
        }
        let xy: Vec<[f64; 2]> = bp[a..=end].iter().map(|p| plane.xy(*p)).collect();
        let own_v: Vec<usize> = simplify(&xy, SIMPLIFY_M, &forced)
            .into_iter()
            .map(|i| i + a)
            .filter(|&i| i > a && (i < end || tail_legs == 0))
            .collect();
        let x: Vec<f32> = own_v.iter().map(|&i| xy[i - a][0] as f32).collect();
        let y: Vec<f32> = own_v.iter().map(|&i| xy[i - a][1] as f32).collect();
        let s: Vec<f32> = own_v.iter().map(|&i| bc[i] as f32).collect();
        let kp = sample_profile(bpr, pa, pend, tail_legs == 0, &forced_p);
        let prof = [
            kp.iter().map(|&i| bpr.dist_m[i] as f32).collect(),
            kp.iter().map(|&i| bpr.out_s[i] as f32).collect(),
            kp.iter().map(|&i| bpr.back_s[i] as f32).collect(),
        ];
        // Own segments: from the loop's last vertex at the head to its first at the tail.
        let own_end = head + x.len();
        let at = |i: usize| -> [f32; 2] {
            if i < head {
                [base.x[i], base.y[i]]
            } else if i < own_end {
                [x[i - head], y[i - head]]
            } else {
                let j = sh.tail_from as usize + i - own_end;
                [base.x[j], base.y[j]]
            }
        };
        let last_seg = if tail_legs > 0 { own_end } else { own_end - 1 };
        let segs: Vec<(u32, [f32; 2], [f32; 2])> = (head - 1..last_seg)
            .map(|k| (k as u32, at(k), at(k + 1)))
            .collect();
        let (grid, start, segs) = grid_of(&segs, &[]);
        Some(Self {
            id,
            s_div: s_div as f32,
            scale: scale_of(course),
            length: cb.line().length_m() as f32,
            shared: sh,
            x,
            y,
            s,
            decisions: decisions_of(anns),
            prof,
            grid,
            start,
            segs,
            kept: Vec::new(),
            kept_prof: Vec::new(),
        })
    }

    /// Flat bytes of the route block.
    pub fn to_bytes(&self, out: &mut Vec<u8>) {
        let u = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
        let f = |out: &mut Vec<u8>, v: f32| out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&self.id.to_le_bytes());
        f(out, self.s_div);
        f(out, self.scale);
        f(out, self.length);
        u(out, self.x.len() as u32);
        u(out, self.decisions.len() as u32);
        u(out, self.prof[0].len() as u32);
        let (nx, ny, cell, x0, y0) = self.grid;
        u(out, nx);
        u(out, ny);
        f(out, cell);
        f(out, x0);
        f(out, y0);
        u(out, self.segs.len() as u32);
        let sh = &self.shared;
        u(out, sh.head);
        u(out, sh.tail_from);
        u(out, sh.tail);
        f(out, sh.tail_ds);
        u(out, sh.prof_head);
        u(out, sh.prof_tail_from);
        u(out, sh.prof_tail);
        for v in sh.prof_shift {
            f(out, v);
        }
        for v in self.x.iter().chain(&self.y).chain(&self.s) {
            f(out, *v);
        }
        for d in &self.decisions {
            f(out, d.s_trigger);
            f(out, d.s_point);
            f(out, d.s_until);
            u(out, d.ann);
            u(out, d.src);
            out.push(priority_u8(d.priority));
            out.push(d.kind as u8);
            out.extend_from_slice(&[0, 0]);
        }
        for v in self.prof.iter().flatten() {
            f(out, *v);
        }
        for v in self.start.iter().chain(&self.segs) {
            u(out, *v);
        }
    }

    /// The route alone as a flat block (a route computed on the phone).
    pub fn bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.to_bytes(&mut v);
        v
    }
}

/// The table of a plan: origin and routes.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Origin of the plane (the plan's start).
    pub origin: LonLat,
    /// Routes: the loop then each plan B.
    pub routes: Vec<RouteTable>,
}

impl Track {
    /// Flat bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut blocks: Vec<Vec<u8>> = self.routes.iter().map(RouteTable::bytes).collect();
        let head = 8 + 16 + 8 + 4 * blocks.len();
        let mut out = Vec::with_capacity(head + blocks.iter().map(Vec::len).sum::<usize>());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.origin.lon.to_le_bytes());
        out.extend_from_slice(&self.origin.lat.to_le_bytes());
        out.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut off = head;
        for b in &blocks {
            out.extend_from_slice(&(off as u32).to_le_bytes());
            off += b.len();
        }
        for b in blocks.iter_mut() {
            out.append(b);
        }
        out
    }
}

#[inline]
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[inline]
fn rd_f32(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// A view on the flat table of a plan.
#[derive(Debug, Clone, Copy)]
pub struct TrackView<'a> {
    b: &'a [u8],
    /// Origin of the plane.
    pub origin: LonLat,
    n: usize,
}

impl<'a> TrackView<'a> {
    /// Checks the header and every route block once.
    pub fn new(b: &'a [u8]) -> Result<Self> {
        let bad = |m: &str| Error::Invalid(format!("track: {m}"));
        if b.len() < 32 || &b[..8] != MAGIC {
            return Err(bad("magic"));
        }
        let lon = f64::from_le_bytes(b[8..16].try_into().expect("8 bytes"));
        let lat = f64::from_le_bytes(b[16..24].try_into().expect("8 bytes"));
        let n = rd_u32(b, 24) as usize;
        if b.len() < 32 + 4 * n {
            return Err(bad("route offsets"));
        }
        let v = Self {
            b,
            origin: LonLat::new(lon, lat),
            n,
        };
        for i in 0..n {
            v.route(i)?;
        }
        Ok(v)
    }

    /// Number of routes.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether there is no route.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The route of id `id` (0 the loop, i+1 plan B i; plans B that do not exist have
    /// none).
    pub fn find(&self, id: i32) -> Option<RouteView<'a>> {
        (0..self.n)
            .filter_map(|i| self.route(i).ok())
            .find(|r| r.id() == id)
    }

    fn block(&self, i: usize) -> Result<&'a [u8]> {
        if i >= self.n {
            return Err(Error::Invalid(format!("track: no route {i}")));
        }
        let start = rd_u32(self.b, 32 + 4 * i) as usize;
        let end = if i + 1 < self.n {
            rd_u32(self.b, 32 + 4 * (i + 1)) as usize
        } else {
            self.b.len()
        };
        if start > end || end > self.b.len() {
            return Err(Error::Invalid("track: route bounds".into()));
        }
        Ok(&self.b[start..end])
    }

    /// Route at position `i` in the file (the loop first; a plan B refers to it).
    pub fn route(&self, i: usize) -> Result<RouteView<'a>> {
        let b = self.block(i)?;
        if i == 0 {
            return RouteView::new(b);
        }
        let base = RouteView::sized(self.block(0)?, None)?;
        let v = RouteView::sized(b, Some(&base))?;
        v.check()?;
        Ok(v)
    }
}

/// Projection of `q` on the segment from `a` to `b` (plane position, distance along):
/// (distance along, distance from `q`, position on the segment from 0 to 1).
#[inline]
fn seg_t(a: [f32; 3], b: [f32; 3], q: [f32; 2]) -> (f32, f32, f32) {
    let ([ax, ay, s0], [bx, by, s1]) = (a, b);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((q[0] - ax) * dx + (q[1] - ay) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = (q[0] - ax - t * dx).hypot(q[1] - ay - t * dy);
    (s0 + t * (s1 - s0), d, t)
}

/// The loop's block seen by a plan B (see the module).
#[derive(Debug, Clone, Copy)]
struct Base<'a> {
    b: &'a [u8],
    n: u32,
    o_prof: u32,
    np: u32,
}

/// A view on one route block.
#[derive(Debug, Clone, Copy)]
pub struct RouteView<'a> {
    b: &'a [u8],
    base: Base<'a>,
    /// Vertices in all, own, at the head; first one of the tail in the loop.
    n: usize,
    own_end: usize,
    head: usize,
    tail_from: usize,
    tail_ds: f32,
    /// Profile stations in all, own, at the head; first one of the tail in the loop.
    np: usize,
    p_own_end: usize,
    p_head: usize,
    p_tail_from: usize,
    p_shift: [f32; 3],
    nd: usize,
    nx: u32,
    ny: u32,
    cell: f32,
    x0: f32,
    y0: f32,
    /// Own vertices and the offsets of their `y` and `s` (`x` at the header's end).
    n_own: usize,
    o_y: usize,
    o_s: usize,
    o_dec: usize,
    o_prof: usize,
    np_own: usize,
    o_start: usize,
    o_segs: usize,
    nseg: usize,
}

impl<'a> RouteView<'a> {
    /// View of a route block that does not refer to the loop (the loop, a route computed
    /// on the phone): checks its sizes and its index.
    pub fn new(b: &'a [u8]) -> Result<Self> {
        let v = Self::sized(b, None)?;
        v.check()?;
        Ok(v)
    }

    /// View of a block built by [`RouteTable::to_bytes`] in this process (a route computed
    /// on the phone): only the sizes are checked, O(1).
    pub fn trusted(b: &'a [u8]) -> Result<Self> {
        Self::sized(b, None)
    }

    fn sized(b: &'a [u8], base: Option<&RouteView<'a>>) -> Result<Self> {
        let bad = |m: &str| Error::Invalid(format!("track route: {m}"));
        if b.len() < ROUTE_HEADER {
            return Err(bad("header"));
        }
        let own = rd_u32(b, 16) as usize;
        let nd = rd_u32(b, 20) as usize;
        let np_own = rd_u32(b, 24) as usize;
        let (nx, ny) = (rd_u32(b, 28), rd_u32(b, 32));
        let nseg = rd_u32(b, 48) as usize;
        let (head, tail_from, tail) = (
            rd_u32(b, 52) as usize,
            rd_u32(b, 56) as usize,
            rd_u32(b, 60) as usize,
        );
        let (p_head, p_tail_from, p_tail) = (
            rd_u32(b, 68) as usize,
            rd_u32(b, 72) as usize,
            rd_u32(b, 76) as usize,
        );
        let o_dec = ROUTE_HEADER + 12 * own;
        let o_prof = o_dec + DECISION * nd;
        let o_start = o_prof + 12 * np_own;
        let o_segs = o_start + 4 * (nx as usize * ny as usize + 1);
        let n = head + own + tail;
        let np = p_head + np_own + p_tail;
        if n < 1 || np < 1 || nx == 0 || ny == 0 || b.len() != o_segs + 4 * nseg {
            return Err(bad("sizes"));
        }
        let shares = head > 0 || tail > 0 || p_head > 0 || p_tail > 0;
        let base = match base {
            _ if !shares => Base {
                b: &[],
                n: 0,
                o_prof: 0,
                np: 0,
            },
            None => return Err(bad("refers to the loop")),
            Some(l) if l.head > 0 || l.p_head > 0 || l.own_end != l.n || l.p_own_end != l.np => {
                return Err(bad("loop refers to another route"));
            }
            Some(l) => {
                if head < 1
                    || head > l.n
                    || (tail > 0 && (tail_from < 1 || tail_from + tail != l.n))
                    || p_head < 1
                    || p_head > l.np
                    || (p_tail > 0 && (p_tail_from < 1 || p_tail_from + p_tail != l.np))
                {
                    return Err(bad("shared parts"));
                }
                Base {
                    b: l.b,
                    n: l.n as u32,
                    o_prof: l.o_prof as u32,
                    np: l.np as u32,
                }
            }
        };
        Ok(Self {
            b,
            base,
            n,
            own_end: head + own,
            head,
            tail_from,
            tail_ds: rd_f32(b, 64),
            np,
            p_own_end: p_head + np_own,
            p_head,
            p_tail_from,
            p_shift: [rd_f32(b, 80), rd_f32(b, 84), rd_f32(b, 88)],
            nd,
            nx,
            ny,
            cell: rd_f32(b, 36),
            x0: rd_f32(b, 40),
            y0: rd_f32(b, 44),
            n_own: own,
            o_y: ROUTE_HEADER + 4 * own,
            o_s: ROUTE_HEADER + 8 * own,
            o_dec,
            o_prof,
            np_own,
            o_start,
            o_segs,
            nseg,
        })
    }

    fn check(&self) -> Result<()> {
        let bad = |m: &str| Error::Invalid(format!("track route: {m}"));
        let cells = self.nx as usize * self.ny as usize;
        for c in 0..=cells {
            if rd_u32(self.b, self.o_start + 4 * c) as usize > self.nseg {
                return Err(bad("grid"));
            }
        }
        for k in 0..self.nseg {
            if rd_u32(self.b, self.o_segs + 4 * k) as usize + 1 >= self.n {
                return Err(bad("segment"));
            }
        }
        for d in 0..self.nd {
            if self.decision(d).s_trigger.is_nan() {
                return Err(bad("decision"));
            }
        }
        Ok(())
    }

    /// Route id (0 the loop, i+1 plan B i, -1 computed on the phone).
    pub fn id(&self) -> i32 {
        i32::from_le_bytes([self.b[0], self.b[1], self.b[2], self.b[3]])
    }

    /// Where it leaves the loop, metres.
    pub fn s_div(&self) -> f32 {
        rd_f32(self.b, 4)
    }

    /// Profile distance per route distance.
    pub fn scale(&self) -> f32 {
        rd_f32(self.b, 8)
    }

    /// Length, metres.
    pub fn length(&self) -> f32 {
        rd_f32(self.b, 12)
    }

    /// Vertices.
    pub fn points(&self) -> usize {
        self.n
    }

    /// Bytes of the route's own block (what it does not share with the loop).
    pub fn own_bytes(&self) -> usize {
        self.b.len()
    }

    /// Vertex `i`: plane position and distance along. The route's own vertices are read
    /// in place (the loop and a route computed on the phone have only those); a plan B's
    /// shared ones in the loop's block.
    #[inline]
    fn vertex(&self, i: usize) -> [f32; 3] {
        let j = i.wrapping_sub(self.head);
        if j < self.n_own {
            [
                rd_f32(self.b, ROUTE_HEADER + 4 * j),
                rd_f32(self.b, self.o_y + 4 * j),
                rd_f32(self.b, self.o_s + 4 * j),
            ]
        } else {
            self.shared_vertex(i)
        }
    }

    #[inline]
    fn shared_vertex(&self, i: usize) -> [f32; 3] {
        let n = self.base.n as usize;
        let (j, ds) = if i < self.head {
            (i, 0.0)
        } else {
            (self.tail_from + i - self.own_end, self.tail_ds)
        };
        let b = self.base.b;
        [
            rd_f32(b, ROUTE_HEADER + 4 * j),
            rd_f32(b, ROUTE_HEADER + 4 * (n + j)),
            rd_f32(b, ROUTE_HEADER + 4 * (2 * n + j)) + ds,
        ]
    }

    /// Distance along the route of vertex `i`, metres.
    #[inline]
    pub fn s(&self, i: usize) -> f32 {
        let j = i.wrapping_sub(self.head);
        if j < self.n_own {
            rd_f32(self.b, self.o_s + 4 * j)
        } else {
            self.shared_vertex(i)[2]
        }
    }

    /// Plane position of vertex `i`.
    pub fn xy(&self, i: usize) -> [f32; 2] {
        let v = self.vertex(i);
        [v[0], v[1]]
    }

    /// Decisions.
    pub fn decisions(&self) -> usize {
        self.nd
    }

    /// Decision `i` (sorted by trigger distance).
    pub fn decision(&self, i: usize) -> Decision {
        let o = self.o_dec + DECISION * i;
        Decision {
            s_trigger: rd_f32(self.b, o),
            s_point: rd_f32(self.b, o + 4),
            s_until: rd_f32(self.b, o + 8),
            ann: rd_u32(self.b, o + 12),
            src: rd_u32(self.b, o + 16),
            priority: priority_of(self.b[o + 20]),
            kind: DecisionKind::from_u8(self.b[o + 21]),
        }
    }

    /// Profile station `i`: distance, model time out, model time back (A8).
    #[inline]
    pub fn profile(&self, i: usize) -> (f32, f32, f32) {
        let j = i.wrapping_sub(self.p_head);
        if j < self.np_own {
            let (b, o, np) = (self.b, self.o_prof, self.np_own);
            (
                rd_f32(b, o + 4 * j),
                rd_f32(b, o + 4 * (np + j)),
                rd_f32(b, o + 4 * (2 * np + j)),
            )
        } else {
            self.shared_profile(i)
        }
    }

    #[inline(never)]
    fn shared_profile(&self, i: usize) -> (f32, f32, f32) {
        let (j, sh) = if i < self.p_head {
            (i, [0.0; 3])
        } else {
            (self.p_tail_from + i - self.p_own_end, self.p_shift)
        };
        let (b, o, np) = (
            self.base.b,
            self.base.o_prof as usize,
            self.base.np as usize,
        );
        (
            rd_f32(b, o + 4 * j) + sh[0],
            rd_f32(b, o + 4 * (np + j)) + sh[1],
            rd_f32(b, o + 4 * (2 * np + j)) + sh[2],
        )
    }

    /// Stations of the profile.
    pub fn profile_len(&self) -> usize {
        self.np
    }

    /// Model time from the start to a route distance, seconds.
    pub fn model_s_at(&self, s: f32) -> f32 {
        let d = s * self.scale();
        // Binary search on the profile distances.
        let (mut lo, mut hi) = (0usize, self.np - 1);
        if d <= self.profile(0).0 {
            return self.profile(0).1;
        }
        if d >= self.profile(hi).0 {
            return self.profile(hi).1;
        }
        while hi - lo > 1 {
            let m = (lo + hi) / 2;
            if self.profile(m).0 <= d {
                lo = m;
            } else {
                hi = m;
            }
        }
        let (d0, t0, _) = self.profile(lo);
        let (d1, t1, _) = self.profile(hi);
        if d1 > d0 {
            t0 + (t1 - t0) * (d - d0) / (d1 - d0)
        } else {
            t0
        }
    }

    /// The A8 profile (allocates: once per route change).
    pub fn to_profile(&self) -> skimap_assist::suggest::Profile {
        let mut p = skimap_assist::suggest::Profile::default();
        for i in 0..self.np {
            let (d, o, b) = self.profile(i);
            p.dist_m.push(f64::from(d));
            p.out_s.push(f64::from(o));
            p.back_s.push(f64::from(b));
        }
        p
    }

    #[inline]
    fn seg_project(&self, k: usize, q: [f32; 2]) -> (f32, f32) {
        let (s, d, _) = self.seg_project_t(k, q);
        (s, d)
    }

    /// Index of the segment containing distance `s`.
    pub fn segment_at(&self, s: f32) -> usize {
        let (mut lo, mut hi) = (0usize, self.n - 1);
        while hi - lo > 1 {
            let m = (lo + hi) / 2;
            if self.s(m) <= s {
                lo = m;
            } else {
                hi = m;
            }
        }
        lo.min(self.n.saturating_sub(2))
    }

    /// Nearest point of the route between distances `lo` and `hi` (window ahead of a
    /// cursor): (distance along, distance from `q`). No allocation.
    pub fn project_window(&self, q: [f32; 2], lo: f32, hi: f32) -> Option<(f32, f32)> {
        if self.n < 2 {
            return None;
        }
        let mut k = self.segment_at(lo.max(0.0));
        let mut a = self.vertex(k);
        let mut best: Option<(f32, f32)> = None;
        while k + 1 < self.n && a[2] <= hi {
            let b = self.vertex(k + 1);
            let (s, d, _) = seg_t(a, b, q);
            if s >= lo && s <= hi && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((s, d));
            }
            a = b;
            k += 1;
        }
        best
    }

    /// Like [`RouteView::project_window`], but among the passes of the route near `q`
    /// (local minima of the distance along the route) within `slack` metres of the
    /// nearest, the first: a hairpin or an out-and-back puts a later part of the route a
    /// few metres away, the skier reaches the earlier one first. No allocation.
    pub fn project_forward(&self, q: [f32; 2], lo: f32, hi: f32, slack: f32) -> Option<(f32, f32)> {
        let (bs, best) = self.project_window(q, lo, hi)?;
        let mut k = self.segment_at(lo.max(0.0));
        let mut a = self.vertex(k);
        // Distance at the previous segment's end, to find the vertex minima.
        let mut prev_end: Option<f32> = None;
        while k + 1 < self.n && a[2] <= hi {
            let b = self.vertex(k + 1);
            let (s, d, t) = seg_t(a, b, q);
            let interior = t > 0.0 && t < 1.0;
            // A vertex is a minimum when the previous segment ended on it and this one
            // starts on it.
            let vertex = t == 0.0 && prev_end.is_some_and(|e| (e - d).abs() < 1e-3);
            if (interior || vertex) && s >= lo && s <= hi && d <= best + slack {
                return Some((s, d));
            }
            prev_end = (t == 1.0).then_some(d);
            a = b;
            k += 1;
        }
        Some((bs, best))
    }

    #[inline]
    fn seg_project_t(&self, k: usize, q: [f32; 2]) -> (f32, f32, f32) {
        seg_t(self.vertex(k), self.vertex(k + 1), q)
    }

    /// Segments of a grid in the cells around `q`: (block, start offset, segments offset,
    /// segments) of each of the 9 cells.
    #[inline]
    fn cells_around(
        b: &[u8],
        grid: (u32, u32, f32, f32, f32),
        o_start: usize,
        q: [f32; 2],
        mut each: impl FnMut(usize, usize),
    ) {
        let (nx, ny, cell, x0, y0) = grid;
        let ci = ((q[0] - x0) / cell).floor();
        let cj = ((q[1] - y0) / cell).floor();
        for dj in -1..=1i64 {
            for di in -1..=1i64 {
                let (i, j) = (ci as i64 + di, cj as i64 + dj);
                if i < 0 || j < 0 || i >= i64::from(nx) || j >= i64::from(ny) {
                    continue;
                }
                let c = (j as usize) * nx as usize + i as usize;
                each(
                    rd_u32(b, o_start + 4 * c) as usize,
                    rd_u32(b, o_start + 4 * (c + 1)) as usize,
                );
            }
        }
    }

    /// Nearest point of the route within the grid cells around `q` (re-acquisition after
    /// a gap or a deviation), the one nearest to `prefer` on a tie of 5 m. No allocation.
    pub fn project_grid(&self, q: [f32; 2], prefer: f32) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        let consider = |k: usize, best: &mut Option<(f32, f32)>| {
            let (s, d) = self.seg_project(k, q);
            let better = match *best {
                None => true,
                Some((bs, bd)) => {
                    d + 5.0 < bd || (d < bd + 5.0 && (s - prefer).abs() < (bs - prefer).abs())
                }
            };
            if better {
                *best = Some((s, d));
            }
        };
        let grid = (self.nx, self.ny, self.cell, self.x0, self.y0);
        Self::cells_around(self.b, grid, self.o_start, q, |a, b| {
            for idx in a..b.min(self.nseg) {
                let k = rd_u32(self.b, self.o_segs + 4 * idx) as usize;
                consider(k, &mut best);
            }
        });
        if self.head > 1 || self.own_end < self.n {
            // The loop's segments this route shares, in the loop's grid.
            let lb = self.base.b;
            let ln = self.base.n as usize;
            let lnd = rd_u32(lb, 20) as usize;
            let lnp = rd_u32(lb, 24) as usize;
            let (lnx, lny) = (rd_u32(lb, 28), rd_u32(lb, 32));
            let lnseg = rd_u32(lb, 48) as usize;
            let lgrid = (lnx, lny, rd_f32(lb, 36), rd_f32(lb, 40), rd_f32(lb, 44));
            let lo_start = ROUTE_HEADER + 12 * ln + DECISION * lnd + 12 * lnp;
            let lo_segs = lo_start + 4 * (lnx as usize * lny as usize + 1);
            Self::cells_around(lb, lgrid, lo_start, q, |a, b| {
                for idx in a..b.min(lnseg) {
                    let kb = rd_u32(lb, lo_segs + 4 * idx) as usize;
                    let k = if kb + 1 < self.head {
                        kb
                    } else if self.own_end < self.n && kb >= self.tail_from && kb + 1 < ln {
                        kb - self.tail_from + self.own_end
                    } else {
                        continue;
                    };
                    consider(k, &mut best);
                }
            });
        }
        best
    }

    /// Plane position at a distance.
    pub fn point_at(&self, s: f32) -> [f32; 2] {
        let k = self.segment_at(s);
        let ([x0, y0, s0], [x1, y1, s1]) = (self.vertex(k), self.vertex(k + 1));
        let t = if s1 > s0 {
            ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        [x0 + t * (x1 - x0), y0 + t * (y1 - y0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::PlanLeg;
    use crate::course::tests::{plane, square, square_loop};
    use skimap_core::eta::EtaModel;
    use skimap_core::routing::Style;

    fn turn() -> crate::plan::PlannedAnnouncement {
        crate::plan::PlannedAnnouncement {
            key: "turn.0".into(),
            kind: "turn".into(),
            trigger: Trigger::At {
                s: 850.0,
                until: 1030.0,
            },
            priority: Priority::Direction,
            event: Some(skimap_voice::Event::Turn {
                metres: 150.0,
                direction: skimap_voice::clips::Direction::Left,
                piste: None,
            }),
            announcement: Default::default(),
            text: String::new(),
        }
    }

    #[test]
    fn flat_table_round_trip_projection_and_index() {
        let g = square();
        let c = Course::new(&g, &square_loop(&g), &EtaModel::new(Style::Classic)).unwrap();
        let pl = plane();
        let a = turn();
        let r = RouteTable::new(0, 0.0, &c, &pl, &[(0, 0, &a)]);
        // 4 straight sides of 20 vertices each: only the corners are kept.
        assert_eq!(r.x.len(), 5);
        let t = Track {
            origin: LonLat::new(6.1, 46.6),
            routes: vec![r.clone(), r],
        };
        let bytes = t.to_bytes();
        let v = TrackView::new(&bytes).unwrap();
        assert_eq!(v.len(), 2);
        let rv = v.route(1).unwrap();
        assert_eq!(rv.points(), 5);
        assert!((rv.length() - 4000.0).abs() < 5.0);
        let d = rv.decision(0);
        assert_eq!(
            (d.s_trigger, d.s_point, d.priority, d.kind),
            (850.0, 1000.0, Priority::Direction, DecisionKind::Turn)
        );
        // 30 m north of the south side at x = 500.
        let q = [500.0, 30.0];
        let (s, dd) = rv.project_window(q, 0.0, 600.0).unwrap();
        assert!(
            (s - 500.0).abs() < 2.0 && (dd - 30.0).abs() < 0.5,
            "{s} {dd}"
        );
        let (s, dd) = rv.project_grid(q, 0.0).unwrap();
        assert!(
            (s - 500.0).abs() < 2.0 && (dd - 30.0).abs() < 0.5,
            "{s} {dd}"
        );
        // Same projection as the line of the course.
        let ll = pl.lonlat([500.0, 30.0]);
        let (s2, d2) = c.line().project(ll, 0.0, 4000.0).unwrap();
        assert!((f64::from(s) - s2).abs() < 2.0 && (f64::from(dd) - d2).abs() < 0.5);
        assert!((rv.model_s_at(2000.0) as f64 - c.model_s() / 2.0).abs() < 2.0);
        // Far away: nothing in the cells around.
        assert!(rv.project_grid([5000.0, 5000.0], 0.0).is_none());
        // Corrupt bytes are refused.
        let mut bad = bytes.clone();
        bad.truncate(bad.len() - 3);
        assert!(TrackView::new(&bad).is_err());
        assert!(TrackView::new(b"nothing").is_err());
    }

    /// Plans B on the square loop, read through the loop's block, are the same routes as
    /// written whole (X13): across the diagonal from the start then the loop's last two
    /// sides (a shared tail), the loop's first two sides then back across the diagonal (a
    /// shared head), the first side, out and back on the spur, then the loop (both).
    #[test]
    fn a_plan_b_shares_the_loop_and_reads_the_same() {
        let g = square();
        let eta = EtaModel::new(Style::Classic);
        let legs = square_loop(&g);
        let c = Course::new(&g, &legs, &eta).unwrap();
        let pl = plane();
        let leg = |e: usize, fwd: bool| PlanLeg {
            edge: e,
            from_m: if fwd { 0.0 } else { g.length_m(e) },
            to_m: if fwd { g.length_m(e) } else { 0.0 },
        };
        let cases: Vec<(Vec<PlanLeg>, usize, usize)> = vec![
            (vec![leg(4, true), legs[2], legs[3]], 0, 2),
            (vec![legs[0], legs[1], leg(4, false)], 2, 0),
            (
                vec![
                    legs[0],
                    leg(5, true),
                    leg(5, false),
                    legs[1],
                    legs[2],
                    legs[3],
                ],
                1,
                3,
            ),
        ];
        let base = RouteTable::new(0, 0.0, &c, &pl, &[]);
        for (b_legs, h, tl) in &cases {
            let cb = Course::new(&g, b_legs, &eta).unwrap();
            let s_div = c.leg_bounds()[*h].0;
            let whole = RouteTable::new(1, s_div, &cb, &pl, &[]);
            let shared = RouteTable::on_loop(&base, &c, 1, s_div, &cb, &pl, &[], *h, *tl)
                .expect("shares the loop");
            assert!(shared.shared.head > 0, "{:?}", shared.shared);
            assert_eq!(shared.shared.tail > 0, *tl > 0, "{:?}", shared.shared);
            let t = Track {
                origin: LonLat::new(6.1, 46.6),
                routes: vec![base.clone(), shared.clone()],
            };
            let w = Track {
                origin: LonLat::new(6.1, 46.6),
                routes: vec![base.clone(), whole],
            };
            let (tb, wb) = (t.to_bytes(), w.to_bytes());
            let (tv, wv) = (TrackView::new(&tb).unwrap(), TrackView::new(&wb).unwrap());
            let (a, b) = (tv.route(1).unwrap(), wv.route(1).unwrap());
            assert!(
                a.own_bytes() < b.own_bytes(),
                "{} vs {}",
                a.own_bytes(),
                b.own_bytes()
            );
            assert!((a.length() - b.length()).abs() < 1e-3);
            // Same geometry along and the same model times.
            for s in (0..=(b.length() as i32)).step_by(25) {
                let s = s as f32;
                let (pa, pb) = (a.point_at(s), b.point_at(s));
                assert!(
                    (pa[0] - pb[0]).hypot(pa[1] - pb[1]) < 2.0 * SIMPLIFY_M as f32,
                    "{s}: {pa:?} {pb:?}"
                );
                assert!((a.model_s_at(s) - b.model_s_at(s)).abs() < 1.0, "{s}");
            }
            assert!((f64::from(a.model_s_at(a.length())) - cb.model_s()).abs() < 0.5);
            let (pa, pb) = (a.to_profile(), b.to_profile());
            assert!((pa.length_m() - pb.length_m()).abs() < 0.5);
            assert!((pa.total_s() - pb.total_s()).abs() < 0.5);
            assert!((pa.back_s.last().unwrap() - pb.back_s.last().unwrap()).abs() < 0.5);
            // Projections, in the window and in the grid (shared head, own, shared tail).
            for q in [
                [300.0, 20.0],
                [700.0, 690.0],
                [20.0, 300.0],
                [980.0, 15.0],
                [1300.0, 10.0],
                [500.0, 1010.0],
            ] {
                let (x, y) = (a.project_grid(q, 0.0), b.project_grid(q, 0.0));
                match (x, y) {
                    (Some((sa, da)), Some((sb, db))) => assert!(
                        (da - db).abs() < 2.5 && (sa - sb).abs() < 5.0,
                        "{q:?}: {x:?} {y:?}"
                    ),
                    _ => assert_eq!(x.is_some(), y.is_some(), "{q:?}"),
                }
                let (x, y) = (
                    a.project_window(q, 0.0, a.length()),
                    b.project_window(q, 0.0, b.length()),
                );
                let ((sa, da), (sb, db)) = (x.unwrap(), y.unwrap());
                assert!(
                    (da - db).abs() < 2.5 && (sa - sb).abs() < 5.0,
                    "{q:?}: {x:?} {y:?}"
                );
            }
            // A plan B cannot be read without the loop.
            assert!(RouteView::new(&shared.bytes()).is_err());
        }
    }
}
