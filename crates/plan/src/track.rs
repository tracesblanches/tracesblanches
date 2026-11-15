//! What the phone reads on every fix, computed on the workstation when the plan is made
//! (E1 #125): for the loop and each plan B, the route geometry in a local plane, a grid
//! index of its segments, the decision points (trigger, point, end) sorted along it, and
//! the model time profile. Stored flat (little endian, 4-byte aligned) and read in place
//! by [`TrackView`] / [`RouteView`]: no parse, no copy, no allocation.
//!
//! # File (`plan_<…>.trk`)
//!
//! ```text
//! magic "SKTRK001", origin lon f64, lat f64, routes u32, 0 u32, offset u32 per route
//! route: id i32 (0 loop, i+1 plan B i, -1 computed on the phone), s_div f32, scale f32,
//!        length f32, points u32, decisions u32, profile u32, nx u32, ny u32, cell f32,
//!        x0 f32, y0 f32, index u32
//!        x f32[points], y f32[points], s f32[points]
//!        decision[decisions]: s_trigger f32, s_point f32, s_until f32, ann u32, src u32,
//!                             priority u8, kind u8, 0 u16
//!        profile: dist f32[profile], out f32[profile], back f32[profile]
//!        grid: start u32[nx·ny + 1], segment u32[index]
//! ```
//!
//! Coordinates are metres east and north of the origin (the plan's start) on the local
//! plane of `skimap_core::graph::LocalPlane`; `s` is the distance along the route of the
//! [`crate::course::Course`] (haversine), kept at every vertex left by the simplification
//! (Douglas–Peucker, [`SIMPLIFY_M`]). `scale` turns `s` into the distance of the time
//! profile (A8 works on the profile). A decision's announcement is `ann` in the list
//! `src` of the plan (0: the loop's `announcements`, i+1: plan B i's).

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
const MAGIC: &[u8; 8] = b"SKTRK001";
const ROUTE_HEADER: usize = 13 * 4;
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
            _ => DecisionKind::Other,
        }
    }

    fn from_u8(b: u8) -> Self {
        match b {
            0 => DecisionKind::Turn,
            1 => DecisionKind::NoNetwork,
            2 => DecisionKind::NoNetworkEnd,
            3 => DecisionKind::Avalanche,
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
    x: Vec<f32>,
    y: Vec<f32>,
    s: Vec<f32>,
    decisions: Vec<Decision>,
    prof: [Vec<f32>; 3],
    grid: (u32, u32, f32, f32, f32),
    start: Vec<u32>,
    segs: Vec<u32>,
}

/// Douglas–Peucker on plane points, keeping the first and last: indices kept.
fn simplify(xy: &[[f64; 2]], tol: f64) -> Vec<usize> {
    let n = xy.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
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
        let kept = simplify(&xy, SIMPLIFY_M);
        let cum = line.cumulative();
        let x: Vec<f32> = kept.iter().map(|&i| xy[i][0] as f32).collect();
        let y: Vec<f32> = kept.iter().map(|&i| xy[i][1] as f32).collect();
        let s: Vec<f32> = kept.iter().map(|&i| cum[i] as f32).collect();
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
        // Time profile: one station every PROFILE_M at least (and the last).
        let p = course.profile();
        let mut keep: Vec<usize> = Vec::new();
        for i in 0..p.dist_m.len() {
            let last = i + 1 == p.dist_m.len();
            if keep
                .last()
                .is_none_or(|&k| p.dist_m[i] - p.dist_m[k] >= PROFILE_M)
                || last
            {
                keep.push(i);
            }
        }
        let prof = [
            keep.iter().map(|&i| p.dist_m[i] as f32).collect(),
            keep.iter().map(|&i| p.out_s[i] as f32).collect(),
            keep.iter().map(|&i| p.back_s[i] as f32).collect(),
        ];
        // Grid of the segments (by bounding box, one cell of margin around).
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..x.len() {
            x0 = x0.min(x[i]);
            y0 = y0.min(y[i]);
            x1 = x1.max(x[i]);
            y1 = y1.max(y[i]);
        }
        let cell = CELL_M as f32;
        x0 -= cell;
        y0 -= cell;
        let nx = (((x1 + cell - x0) / cell).ceil() as u32).max(1);
        let ny = (((y1 + cell - y0) / cell).ceil() as u32).max(1);
        let mut cells: Vec<Vec<u32>> = vec![Vec::new(); (nx * ny) as usize];
        for k in 0..x.len().saturating_sub(1) {
            let (ax, bx) = (x[k].min(x[k + 1]), x[k].max(x[k + 1]));
            let (ay, by) = (y[k].min(y[k + 1]), y[k].max(y[k + 1]));
            let cx = |v: f32| (((v - x0) / cell) as u32).min(nx - 1);
            let cy = |v: f32| (((v - y0) / cell) as u32).min(ny - 1);
            for j in cy(ay)..=cy(by) {
                for i in cx(ax)..=cx(bx) {
                    cells[(j * nx + i) as usize].push(k as u32);
                }
            }
        }
        let mut start = Vec::with_capacity(cells.len() + 1);
        let mut segs = Vec::new();
        for c in &cells {
            start.push(segs.len() as u32);
            segs.extend_from_slice(c);
        }
        start.push(segs.len() as u32);
        let scale = if line.length_m() > 0.0 {
            (p.length_m() / line.length_m()) as f32
        } else {
            1.0
        };
        Self {
            id,
            s_div: s_div as f32,
            scale,
            length: line.length_m() as f32,
            x,
            y,
            s,
            decisions,
            prof,
            grid: (nx, ny, cell, x0, y0),
            start,
            segs,
        }
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

    /// Route at position `i` in the file (the loop first).
    pub fn route(&self, i: usize) -> Result<RouteView<'a>> {
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
        RouteView::new(&self.b[start..end])
    }
}

/// A view on one route block.
#[derive(Debug, Clone, Copy)]
pub struct RouteView<'a> {
    b: &'a [u8],
    n: usize,
    nd: usize,
    np: usize,
    nx: u32,
    ny: u32,
    cell: f32,
    x0: f32,
    y0: f32,
    o_x: usize,
    o_y: usize,
    o_s: usize,
    o_dec: usize,
    o_prof: usize,
    o_start: usize,
    o_segs: usize,
    nseg: usize,
}

impl<'a> RouteView<'a> {
    /// View of a route block (checks its sizes and its index).
    pub fn new(b: &'a [u8]) -> Result<Self> {
        let v = Self::sized(b)?;
        v.check()?;
        Ok(v)
    }

    /// View of a block built by [`RouteTable::to_bytes`] in this process (a route computed
    /// on the phone): only the sizes are checked, O(1).
    pub fn trusted(b: &'a [u8]) -> Result<Self> {
        Self::sized(b)
    }

    fn sized(b: &'a [u8]) -> Result<Self> {
        let bad = |m: &str| Error::Invalid(format!("track route: {m}"));
        if b.len() < ROUTE_HEADER {
            return Err(bad("header"));
        }
        let n = rd_u32(b, 16) as usize;
        let nd = rd_u32(b, 20) as usize;
        let np = rd_u32(b, 24) as usize;
        let (nx, ny) = (rd_u32(b, 28), rd_u32(b, 32));
        let nseg = rd_u32(b, 48) as usize;
        let o_x = ROUTE_HEADER;
        let o_y = o_x + 4 * n;
        let o_s = o_y + 4 * n;
        let o_dec = o_s + 4 * n;
        let o_prof = o_dec + DECISION * nd;
        let o_start = o_prof + 12 * np;
        let o_segs = o_start + 4 * (nx as usize * ny as usize + 1);
        if n < 1 || np < 1 || nx == 0 || ny == 0 || b.len() != o_segs + 4 * nseg {
            return Err(bad("sizes"));
        }
        let v = Self {
            b,
            n,
            nd,
            np,
            nx,
            ny,
            cell: rd_f32(b, 36),
            x0: rd_f32(b, 40),
            y0: rd_f32(b, 44),
            o_x,
            o_y,
            o_s,
            o_dec,
            o_prof,
            o_start,
            o_segs,
            nseg,
        };
        Ok(v)
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

    #[inline]
    fn x(&self, i: usize) -> f32 {
        rd_f32(self.b, self.o_x + 4 * i)
    }

    #[inline]
    fn y(&self, i: usize) -> f32 {
        rd_f32(self.b, self.o_y + 4 * i)
    }

    /// Distance along the route of vertex `i`, metres.
    #[inline]
    pub fn s(&self, i: usize) -> f32 {
        rd_f32(self.b, self.o_s + 4 * i)
    }

    /// Plane position of vertex `i`.
    pub fn xy(&self, i: usize) -> [f32; 2] {
        [self.x(i), self.y(i)]
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
    pub fn profile(&self, i: usize) -> (f32, f32, f32) {
        (
            rd_f32(self.b, self.o_prof + 4 * i),
            rd_f32(self.b, self.o_prof + 4 * (self.np + i)),
            rd_f32(self.b, self.o_prof + 4 * (2 * self.np + i)),
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
        let (ax, ay, bx, by) = (self.x(k), self.y(k), self.x(k + 1), self.y(k + 1));
        let (dx, dy) = (bx - ax, by - ay);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((q[0] - ax) * dx + (q[1] - ay) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = (q[0] - ax - t * dx).hypot(q[1] - ay - t * dy);
        let (s0, s1) = (self.s(k), self.s(k + 1));
        (s0 + t * (s1 - s0), d)
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
        let mut best: Option<(f32, f32)> = None;
        while k + 1 < self.n && self.s(k) <= hi {
            let (s, d) = self.seg_project(k, q);
            if s >= lo && s <= hi && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((s, d));
            }
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
        // Distance at the previous segment's end, to find the vertex minima.
        let mut prev_end: Option<f32> = None;
        while k + 1 < self.n && self.s(k) <= hi {
            let (s, d, t) = self.seg_project_t(k, q);
            let interior = t > 0.0 && t < 1.0;
            // A vertex is a minimum when the previous segment ended on it and this one
            // starts on it.
            let vertex = t == 0.0 && prev_end.is_some_and(|e| (e - d).abs() < 1e-3);
            if (interior || vertex) && s >= lo && s <= hi && d <= best + slack {
                return Some((s, d));
            }
            prev_end = (t == 1.0).then_some(d);
            k += 1;
        }
        Some((bs, best))
    }

    #[inline]
    fn seg_project_t(&self, k: usize, q: [f32; 2]) -> (f32, f32, f32) {
        let (ax, ay, bx, by) = (self.x(k), self.y(k), self.x(k + 1), self.y(k + 1));
        let (dx, dy) = (bx - ax, by - ay);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((q[0] - ax) * dx + (q[1] - ay) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = (q[0] - ax - t * dx).hypot(q[1] - ay - t * dy);
        let (s0, s1) = (self.s(k), self.s(k + 1));
        (s0 + t * (s1 - s0), d, t)
    }

    /// Nearest point of the route within the grid cells around `q` (re-acquisition after
    /// a gap or a deviation), the one nearest to `prefer` on a tie of 5 m. No allocation.
    pub fn project_grid(&self, q: [f32; 2], prefer: f32) -> Option<(f32, f32)> {
        let ci = ((q[0] - self.x0) / self.cell).floor();
        let cj = ((q[1] - self.y0) / self.cell).floor();
        let mut best: Option<(f32, f32)> = None;
        for dj in -1..=1i64 {
            for di in -1..=1i64 {
                let (i, j) = (ci as i64 + di, cj as i64 + dj);
                if i < 0 || j < 0 || i >= i64::from(self.nx) || j >= i64::from(self.ny) {
                    continue;
                }
                let c = (j as usize) * self.nx as usize + i as usize;
                let (a, b) = (
                    rd_u32(self.b, self.o_start + 4 * c) as usize,
                    rd_u32(self.b, self.o_start + 4 * (c + 1)) as usize,
                );
                for idx in a..b.min(self.nseg) {
                    let k = rd_u32(self.b, self.o_segs + 4 * idx) as usize;
                    let (s, d) = self.seg_project(k, q);
                    let better = match best {
                        None => true,
                        Some((bs, bd)) => {
                            d + 5.0 < bd
                                || (d < bd + 5.0 && (s - prefer).abs() < (bs - prefer).abs())
                        }
                    };
                    if better {
                        best = Some((s, d));
                    }
                }
            }
        }
        best
    }

    /// Plane position at a distance.
    pub fn point_at(&self, s: f32) -> [f32; 2] {
        let k = self.segment_at(s);
        let (s0, s1) = (self.s(k), self.s(k + 1));
        let t = if s1 > s0 {
            ((s - s0) / (s1 - s0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        [
            self.x(k) + t * (self.x(k + 1) - self.x(k)),
            self.y(k) + t * (self.y(k + 1) - self.y(k)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::tests::{plane, square, square_loop};
    use skimap_core::eta::EtaModel;
    use skimap_core::routing::Style;

    #[test]
    fn flat_table_round_trip_projection_and_index() {
        let g = square();
        let c = Course::new(&g, &square_loop(&g), &EtaModel::new(Style::Classic)).unwrap();
        let pl = plane();
        let a = crate::plan::PlannedAnnouncement {
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
        };
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
}
