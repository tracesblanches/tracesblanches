//! Reading sheet detected from the plan alone (no hand-written [`PlanSpec`]): resolution
//! and label source from the PDF, inks from the legend (colour of the dash beside each
//! piste, colour words) refined on the lines of the map, line parameters from the measured
//! line width, map area from the drawn network, masks from the flat boxes (title, panels,
//! advertisements, departure boxes) and legend sectors from their headings.
//!
//! Every threshold is in page points or CIELAB units: the same rules read a vector plan at
//! 200 dpi and a picture plan upsampled to 300 dpi.

use std::collections::BTreeMap;

use rayon::prelude::*;

use serde::Serialize;
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::BBox;
use skimap_common::layout::DataLayout;
use skimap_osm::extract::Planet;

use crate::build::{Registration, register_input};
use crate::error::Result;
use crate::image::{Rgb, lab};
use crate::input::{self, Probe, TextBox};
use crate::labels::{fold, join_baselines, parse_colour_line, similarity, split_length};
use crate::segment::{self, LineParams, Mask};
use crate::spec::{Ink, LabelSource, PageRect, PlanSource, PlanSpec};
use crate::transcript::PisteKind;

/// Rendering resolution: twice the native resolution of a picture plan (the OCR reads
/// small letters better upsampled), about 4 000 pixels on the long side of a vector plan;
/// rounded to 50 dpi.
pub fn choose_dpi(p: &Probe) -> u32 {
    let round50 = |v: f64| ((v / 50.0).round() * 50.0) as u32;
    match p.page_image().filter(|_| p.picture_only()) {
        Some(img) => round50(2.0 * img.ppi).clamp(150, 400),
        None => {
            let long_in = p.page_pt[0].max(p.page_pt[1]) / 72.0;
            if long_in <= 0.0 {
                return 200;
            }
            round50(4000.0 / long_in).clamp(100, 300)
        }
    }
}

/// Labels from the text layer when the page has one (20 words at least), else OCR.
pub fn label_source(p: &Probe) -> LabelSource {
    if p.words >= 20 {
        LabelSource::TextLayer
    } else {
        LabelSource::Ocr
    }
}

/// A line of the legend listing one piste, before the inks are known.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntryCand {
    /// Text of the line (joined continuation included).
    pub text: String,
    /// Box, page points.
    pub rect: PageRect,
    /// Ink named by a colour word (« Piste verte : … »).
    pub word_ink: Option<&'static str>,
    /// Kind said by the line (liaison, multi-activities).
    pub kind: Option<PisteKind>,
    /// Colour of the dash left of the line.
    pub dash: Option<[u8; 3]>,
}

/// Kind said by the words of a legend line.
pub fn kind_words(text: &str) -> Option<PisteKind> {
    let f = fold(text);
    if f.contains("multi") {
        Some(PisteKind::MultiActivity)
    } else if f.split(' ').any(|w| similarity(w, "liaison") >= 0.8) {
        Some(PisteKind::Liaison)
    } else if f.contains("raquette") {
        Some(PisteKind::Snowshoe)
    } else if f.contains("pieton") {
        Some(PisteKind::Pedestrian)
    } else {
        None
    }
}

/// Ink named by the colour word after « Piste » (« Piste verte : … », « Piste de liaison
/// … »); a colour word inside a name (« Combe Verte ») names nothing.
pub fn colour_word_ink(text: &str) -> Option<&'static str> {
    let f = fold(text);
    let mut words = f.split(' ');
    if words.next() != Some("piste") {
        return None;
    }
    let w = match words.next()? {
        "de" => words.next()?,
        w => w,
    };
    [
        ("verte", "green"),
        ("bleue", "blue"),
        ("rouge", "red"),
        ("noire", "black"),
        ("liaison", "orange"),
    ]
    .iter()
    .find(|(c, _)| *c == w)
    .map(|(_, n)| *n)
}

/// Legend lines of a plan: on a text layer, « Name 12,3 km » lines (a column of lengths
/// joined to the name on its left, [`crate::labels::join_lengths`]; with the
/// « (Multi-activités) 11,0 km » continuation joined, as [`crate::labels::legend`]); on an
/// OCR reading, « Piste <couleur> : name • length » lines (as
/// [`crate::labels::legend_ocr`], two columns read as one line split).
pub fn entry_candidates(lines: &[TextBox], src: LabelSource) -> Vec<EntryCand> {
    match src {
        LabelSource::TextLayer => {
            let lines = &crate::labels::join_lengths(&join_baselines(lines));
            let mut out = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                let mut text = l.text.clone();
                let mut rect = l.rect;
                if text.starts_with('(')
                    && let Some(prev) = lines[..i].iter().rev().find(|p| {
                        (p.rect[0] - l.rect[0]).abs() < 2.0
                            && l.rect[1] - p.rect[3] < 4.0
                            && l.rect[1] > p.rect[1]
                    })
                {
                    text = format!("{} {}", prev.text, text);
                    rect = [
                        prev.rect[0],
                        prev.rect[1],
                        rect[2].max(prev.rect[2]),
                        rect[3],
                    ];
                }
                let Some((raw_name, _)) = split_length(&text) else {
                    continue;
                };
                let name = match raw_name.split_once(':') {
                    Some((_, n)) => n.trim().to_owned(),
                    None => raw_name.clone(),
                };
                if name.chars().filter(|c| c.is_alphabetic()).count() < 3 {
                    continue;
                }
                out.push(EntryCand {
                    word_ink: colour_word_ink(&raw_name),
                    kind: kind_words(&raw_name),
                    text,
                    rect,
                    dash: None,
                });
            }
            out
        }
        LabelSource::Ocr => crate::labels::join_lengths(lines)
            .iter()
            .flat_map(crate::labels::split_entries)
            .filter_map(|l| {
                // « Piste verte : … », else « Name 2,9 km » (its dash gives the ink).
                let word_ink = match parse_colour_line(&l.text) {
                    Some((ink, _, _)) => Some(ink),
                    None => {
                        let (name, _) = split_length(&l.text)?;
                        if name.chars().filter(|c| c.is_alphabetic()).count() < 3 {
                            return None;
                        }
                        None
                    }
                };
                Some(EntryCand {
                    word_ink,
                    kind: kind_words(&l.text),
                    text: l.text.clone(),
                    rect: l.rect,
                    dash: None,
                })
            })
            .collect(),
    }
}

/// CIELAB distance.
pub fn de(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn median_rgb(px: &[[u8; 3]]) -> [u8; 3] {
    let mut out = [0u8; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let mut v: Vec<u8> = px.iter().map(|p| p[c]).collect();
        v.sort_unstable();
        *o = v[v.len() / 2];
    }
    out
}

/// Colour of the dash left of a legend line: in the band `[x0 − 16 pt, x0 − 1 pt] ×
/// [y0, y1]`, the pixels far (ΔE > 20) from the band's background (its most frequent
/// colour), reduced to their core (median, then the pixels within ΔE 15 of it). `None`
/// below 4 pixels.
pub fn dash_colour(img: &Rgb, rect: PageRect, pt_per_px: f64) -> Option<[u8; 3]> {
    band_colour(
        img,
        [rect[0] - 16.0, rect[1], rect[0] - 1.0, rect[3]],
        pt_per_px,
    )
}

/// Colour of a legend line's marker: the dash beside it ([`dash_colour`]), else the
/// marker further left on its row ([`crate::labels::row_left`]: a numbered disc, a
/// coloured pictogram of a table).
pub fn marker_colour(
    img: &Rgb,
    rect: PageRect,
    pt_per_px: f64,
    lines: &[TextBox],
) -> Option<[u8; 3]> {
    dash_colour(img, rect, pt_per_px).or_else(|| {
        let left = crate::labels::row_left(lines, rect);
        (left < rect[0] - 16.0)
            .then(|| band_colour(img, [left, rect[1], rect[0] - 1.0, rect[3]], pt_per_px))
            .flatten()
    })
}

/// [`dash_colour`] over a band `[x0, y0, x1, y1]`, page points.
pub fn band_colour(img: &Rgb, band: PageRect, pt_per_px: f64) -> Option<[u8; 3]> {
    let rect = band;
    let to_px = |v: f64| (v / pt_per_px).round().max(0.0) as usize;
    let (x0, x1) = (to_px(rect[0]), to_px(rect[2]).min(img.w));
    let (y0, y1) = (to_px(rect[1]), to_px(rect[3]).min(img.h));
    let mut px = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            px.push(img.get(x, y));
        }
    }
    if px.is_empty() {
        return None;
    }
    let mut freq: std::collections::HashMap<[u8; 3], usize> = Default::default();
    for p in &px {
        *freq.entry([p[0] >> 3, p[1] >> 3, p[2] >> 3]).or_default() += 1;
    }
    let mode = freq.iter().max_by_key(|(k, n)| (**n, **k))?.0;
    let bg: Vec<[u8; 3]> = px
        .iter()
        .copied()
        .filter(|p| [p[0] >> 3, p[1] >> 3, p[2] >> 3] == *mode)
        .collect();
    let bg = lab(median_rgb(&bg));
    let cand: Vec<(f32, [u8; 3])> = px
        .into_iter()
        .map(|p| (de(lab(p), bg), p))
        .filter(|(d, _)| *d > 20.0)
        .collect();
    // Blends of a colour with the background keep its hue: the pixels are grouped by hue
    // (30° bins of CIELAB, greys apart) and the largest group is the dash.
    let group = |p: [u8; 3]| {
        let l = lab(p);
        if l[1].hypot(l[2]) < 15.0 {
            12
        } else {
            (l[2].atan2(l[1]).to_degrees().rem_euclid(360.0) / 30.0) as usize
        }
    };
    let mut counts = [0usize; 13];
    for c in &cand {
        counts[group(c.1)] += 1;
    }
    let best = (0..13).max_by_key(|&g| (counts[g], g))?;
    let mut cand: Vec<(f32, [u8; 3])> = cand.into_iter().filter(|c| group(c.1) == best).collect();
    if cand.len() < 4 {
        return None;
    }
    // The core of a thin, blurred dash: the tenth of its pixels furthest from the
    // background (the rest are blends with it).
    cand.sort_by(|a, b| b.0.total_cmp(&a.0));
    let top: Vec<[u8; 3]> = cand[..cand.len().div_ceil(10).max(3).min(cand.len())]
        .iter()
        .map(|c| c.1)
        .collect();
    let m = lab(median_rgb(&top));
    let core: Vec<[u8; 3]> = top.into_iter().filter(|p| de(lab(*p), m) <= 15.0).collect();
    (!core.is_empty()).then(|| median_rgb(&core))
}

/// Name of a colour by lightness, chroma and hue (CIELAB): black, green, blue, red,
/// orange, yellow, purple; `None` for greys and whites.
pub fn colour_name(c: [u8; 3]) -> Option<&'static str> {
    let l = lab(c);
    let chroma = l[1].hypot(l[2]);
    if l[0] < 30.0 && chroma < 12.0 {
        return Some("black");
    }
    if chroma < 10.0 {
        return None;
    }
    let h = l[2].atan2(l[1]).to_degrees().rem_euclid(360.0);
    Some(match h {
        h if !(45.0..345.0).contains(&h) => "red",
        h if h < 80.0 => "orange",
        h if h < 110.0 => {
            if l[0] >= 75.0 {
                "yellow"
            } else {
                "orange"
            }
        }
        h if h < 170.0 => "green",
        h if h < 290.0 => "blue",
        _ => "purple",
    })
}

/// Difficulty colours of the French scale.
pub const DIFFICULTIES: [&str; 4] = ["green", "blue", "red", "black"];

/// Where an ink of the palette was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InkSource {
    /// Legend dashes or colour words.
    Legend,
    /// Line colours of the drawing itself (a legend without coloured dashes).
    Drawing,
    /// A colour drawn on the map that the legend does not list (non-ski).
    Unlisted,
}

/// An ink of the palette with where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct InkFound {
    /// The ink.
    pub ink: Ink,
    /// Where it was read.
    pub source: InkSource,
    /// Colours of the legend dashes (one per group of dashes within ΔE 15).
    pub legend_colours: Vec<[u8; 3]>,
    /// Legend lines of this ink.
    pub entries: usize,
    /// Map pixels of the ink's lines used for the references.
    pub map_pixels: usize,
}

/// A 4-bit colour bin: pixel count and channel sums.
type ColourBin = (usize, [u64; 3]);

/// Legend lines of one ink being grouped: name from words, dash colours, kinds, lines.
type LegendGroup = (Option<&'static str>, Vec<[u8; 3]>, Vec<PisteKind>, usize);

/// Groups the legend lines by ink: colour word when there is one, else the dash colour
/// (greedy clusters within ΔE 12). Returns the inks (named by their colour words or
/// [`colour_name`]) with the dash colour as provisional reference.
pub fn legend_inks(entries: &[EntryCand]) -> Vec<InkFound> {
    let mut groups: Vec<LegendGroup> = Vec::new();
    for e in entries {
        let slot = match (e.word_ink, e.dash) {
            (Some(w), _) => groups.iter().position(|g| g.0 == Some(w)),
            (None, Some(c)) => groups.iter().position(|g| {
                g.0.is_none() && !g.1.is_empty() && de(lab(median_rgb(&g.1)), lab(c)) <= 12.0
            }),
            (None, None) => continue,
        };
        let i = match slot {
            Some(i) => i,
            None => {
                groups.push((e.word_ink, Vec::new(), Vec::new(), 0));
                groups.len() - 1
            }
        };
        let g = &mut groups[i];
        if let Some(c) = e.dash {
            g.1.push(c);
        }
        if let Some(k) = e.kind {
            g.2.push(k);
        }
        g.3 += 1;
    }
    let mut out: Vec<InkFound> = Vec::new();
    for (word, dashes, kinds, n) in groups {
        let colour = (!dashes.is_empty()).then(|| median_rgb(&dashes));
        // One reference per distinct dash colour (a plan may print one ink lighter in one
        // column: two reds on one plan seen).
        let mut subs: Vec<Vec<[u8; 3]>> = Vec::new();
        for &d in &dashes {
            match subs
                .iter_mut()
                .find(|s| de(lab(median_rgb(s)), lab(d)) <= 15.0)
            {
                Some(s) => s.push(d),
                None => subs.push(vec![d]),
            }
        }
        let colours: Vec<[u8; 3]> = subs.iter().map(|s| median_rgb(s)).collect();
        let Some(name) = word.or_else(|| colour.and_then(colour_name)) else {
            continue;
        };
        // A group of the same name (dashes without colour word and lines « Piste
        // rouge … »): one ink, with the other group's distinct colours as references.
        if let Some(o) = out.iter_mut().find(|o| o.ink.name == name) {
            for c in colours {
                if o.legend_colours.iter().all(|x| de(lab(*x), lab(c)) > 15.0) {
                    o.legend_colours.push(c);
                    o.ink.refs.push(c);
                }
            }
            o.entries += n;
            continue;
        }
        // Kind: the most frequent word (liaison, multi-activities) if it is on most lines
        // of the ink, else a ski piste.
        let kind = [PisteKind::Liaison, PisteKind::MultiActivity]
            .into_iter()
            .find(|k| 2 * kinds.iter().filter(|x| *x == k).count() >= n)
            .unwrap_or(if name == "orange" {
                PisteKind::Liaison
            } else {
                PisteKind::Ski
            });
        let difficulty =
            (kind == PisteKind::Ski && DIFFICULTIES.contains(&name)).then(|| name.to_owned());
        out.push(InkFound {
            ink: Ink {
                name: name.to_owned(),
                refs: colours.clone(),
                max_de: 0.0,
                difficulty,
                kind,
            },
            legend_colours: colours,
            source: InkSource::Legend,
            entries: n,
            map_pixels: 0,
        });
    }
    out
}

/// Difficulty colours a drawing ink may take: green, blue, red. Black is left out: the
/// drawing's black lines (roads, contours, text strokes) cannot be told from black pistes
/// without a legend.
pub const DRAWING_DIFFICULTIES: [&str; 3] = ["green", "blue", "red"];

/// Least share of a colour name's line pixels that one of its colour groups holds to be a
/// reference of its own (a plan drawing light and dark blue pistes: two groups).
pub const DRAWING_GROUP_MIN_SHARE: f64 = 0.15;

/// Line colours of the drawing itself, for a legend without coloured dashes. The pixels
/// that stand out of their surroundings (`contrast` ≥ 20) where `keep` holds and whose
/// colour is named green, blue or red ([`colour_name`]) are grouped by name, then within a
/// name by colour (greedy groups within ΔE 20 around the most frequent 4-bit colour bin
/// left; a group holds [`DRAWING_GROUP_MIN_SHARE`] of the name's pixels and 200 at least).
/// Each group's reference is the median of its fifth of highest contrast (line centres,
/// not the blends with the background); a reference within ΔE [`DRAWING_BACKGROUND_DE`]
/// of a background colour of the map (`backgrounds`, [`map_backgrounds`]) is the texture
/// of the background (shading, water, panels), not an ink. One ski ink per name (French
/// difficulty scale), with its groups as references; the caller keeps those that draw
/// lines.
pub fn drawing_inks(
    img: &Rgb,
    contrast: &[f32],
    keep: &dyn Fn(usize, usize) -> bool,
    backgrounds: &[[f32; 3]],
) -> Vec<InkFound> {
    let mut cache: std::collections::HashMap<[u8; 3], Option<&'static str>> = Default::default();
    let mut by_name: BTreeMap<&'static str, Vec<(f32, [u8; 3])>> = BTreeMap::new();
    for y in 0..img.h {
        for x in 0..img.w {
            let i = y * img.w + x;
            if contrast[i] < 20.0 || !keep(x, y) {
                continue;
            }
            let p = img.get(x, y);
            let name = *cache
                .entry(p)
                .or_insert_with(|| colour_name(p).filter(|n| DRAWING_DIFFICULTIES.contains(n)));
            if let Some(n) = name {
                by_name.entry(n).or_default().push((contrast[i], p));
            }
        }
    }
    let mut out = Vec::new();
    for (name, px) in by_name {
        let min_n = ((DRAWING_GROUP_MIN_SHARE * px.len() as f64).ceil() as usize).max(200);
        if px.len() < min_n {
            continue;
        }
        // 4-bit bins: count and colour sums.
        let bin = |p: [u8; 3]| [p[0] >> 4, p[1] >> 4, p[2] >> 4];
        let mut bins: BTreeMap<[u8; 3], ColourBin> = BTreeMap::new();
        for (_, p) in &px {
            let e = bins.entry(bin(*p)).or_default();
            e.0 += 1;
            for (s, v) in e.1.iter_mut().zip(p) {
                *s += *v as u64;
            }
        }
        let mean = |(n, s): &ColourBin| {
            let n = *n as u64;
            lab([(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8])
        };
        let mut order: Vec<([u8; 3], ColourBin)> = bins.into_iter().collect();
        order.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
        let mut group_of: BTreeMap<[u8; 3], usize> = BTreeMap::new();
        let mut groups = 0usize;
        for i in 0..order.len() {
            if group_of.contains_key(&order[i].0) {
                continue;
            }
            let seed = mean(&order[i].1);
            for (k, v) in &order[i..] {
                if !group_of.contains_key(k) && de(mean(v), seed) <= 20.0 {
                    group_of.insert(*k, groups);
                }
            }
            groups += 1;
        }
        let mut members: Vec<Vec<(f32, [u8; 3])>> = vec![Vec::new(); groups];
        for &(c, p) in &px {
            members[group_of[&bin(p)]].push((c, p));
        }
        let mut refs = Vec::new();
        for mut g in members.into_iter().filter(|g| g.len() >= min_n) {
            g.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let top: Vec<[u8; 3]> = g[..g.len().div_ceil(5)].iter().map(|c| c.1).collect();
            let r = median_rgb(&top);
            if backgrounds
                .iter()
                .all(|b| de(lab(r), *b) > DRAWING_BACKGROUND_DE)
            {
                refs.push(r);
            }
        }
        if refs.is_empty() {
            continue;
        }
        out.push(InkFound {
            ink: Ink {
                name: name.to_owned(),
                refs: refs.clone(),
                max_de: 0.0,
                difficulty: Some(name.to_owned()),
                kind: PisteKind::Ski,
            },
            legend_colours: refs,
            source: InkSource::Drawing,
            entries: 0,
            map_pixels: px.len(),
        });
    }
    out
}

/// Least CIELAB distance of a drawing ink's reference to every background colour of the
/// map (twice the smallest ink threshold of [`detect`]).
pub const DRAWING_BACKGROUND_DE: f32 = 16.0;

/// Least length of thin lines (components of 20 pt of skeleton at least, 4 pt wide at
/// most, as [`measured_width`]) an ink read from the drawing draws to be kept, points.
pub const DRAWING_MIN_LINE_PT: f64 = 200.0;

/// A rectangle of the page in pixels `[x0, y0, x1, y1]` (end exclusive) to points.
fn px_rect(r: [usize; 4], k: f64) -> PageRect {
    [
        r[0] as f64 * k,
        r[1] as f64 * k,
        r[2] as f64 * k,
        r[3] as f64 * k,
    ]
}

/// Flat boxes of the page: regions of one colour (pixels within 8 levels per channel of
/// a seed pixel whose 3 × 3 neighbourhood is uniform) that are rectangles once their holes
/// (text, photos, logos) are filled: filled area at least 90 % of the bounding box and
/// 0.2 % of the page, 16 pt wide and high, the colour covering at least 30 % of it.
/// Panels, title, advertisements and departure boxes.
pub fn flat_boxes(img: &Rgb, pt_per_px: f64) -> Vec<PageRect> {
    let (w, h) = (img.w, img.h);
    let tol = 8i32;
    let close = |a: [u8; 3], b: [u8; 3]| (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= tol);
    let uniform = |x: usize, y: usize| {
        let c = img.get(x, y);
        x > 0
            && y > 0
            && x + 1 < w
            && y + 1 < h
            && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .iter()
                .all(|&(a, b)| img.get(a, b) == c)
    };
    let min_px = 0.002 * (w * h) as f64;
    let min_side = (16.0 / pt_per_px).ceil() as usize;
    let mut region = vec![u32::MAX; w * h];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let mut id = 0u32;
    for start in 0..w * h {
        if region[start] != u32::MAX || !uniform(start % w, start / w) {
            continue;
        }
        let seed = img.get(start % w, start / w);
        let this = id;
        id += 1;
        region[start] = this;
        stack.push(start);
        let (mut n, mut x0, mut y0, mut x1, mut y1) = (0usize, w, h, 0, 0);
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
            let mut push = |j: usize| {
                if region[j] == u32::MAX && close(img.get(j % w, j / w), seed) {
                    region[j] = this;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        let (bw, bh) = (x1 - x0, y1 - y0);
        if (n as f64) < min_px * 0.3 || bw < min_side || bh < min_side {
            continue;
        }
        let mut m = Mask::new(bw, bh);
        for y in y0..y1 {
            for x in x0..x1 {
                m.v[(y - y0) * bw + (x - x0)] = u8::from(region[y * w + x] == this);
            }
        }
        let filled = m.fill_holes(usize::MAX).count() as f64;
        if filled < min_px || filled < 0.9 * (bw * bh) as f64 || (n as f64) < 0.3 * filled {
            continue;
        }
        out.push(px_rect([x0, y0, x1, y1], pt_per_px));
    }
    out
}

/// Bounding box (pixels) of the main drawn network: the kept line pixels grouped by
/// proximity (cells of about 2 pt, joined within 10 pt), the group with the most pixels
/// inside `region` (page points); the group may extend beyond it.
pub fn main_network(kept: &Mask, pt_per_px: f64, region: PageRect) -> Option<[usize; 4]> {
    networks(kept, pt_per_px, region).into_iter().next()
}

/// Smallest share of the main network's line pixels for another network of the page to
/// be a map of its own (pages with several maps).
pub const OTHER_MAP_MIN_SHARE: f64 = 0.25;

/// Every drawn network of the page (bounding boxes, pixels): the main one first
/// ([`main_network`]), then the other groups holding at least [`OTHER_MAP_MIN_SHARE`] of
/// its line pixels, by size: the other maps of a page with several maps.
pub fn networks(kept: &Mask, pt_per_px: f64, region: PageRect) -> Vec<[usize; 4]> {
    let b = ((2.0 / pt_per_px).round() as usize).max(1);
    let (gw, gh) = (kept.w.div_ceil(b), kept.h.div_ceil(b));
    let mut cells = vec![0usize; gw * gh];
    for y in 0..kept.h {
        for x in 0..kept.w {
            if kept.v[y * kept.w + x] != 0 {
                cells[(y / b) * gw + x / b] += 1;
            }
        }
    }
    let mut on = Mask::new(gw, gh);
    for (i, &c) in cells.iter().enumerate() {
        on.v[i] = u8::from(c > 0);
    }
    let r = 5;
    let grown = {
        let mut g = Mask::new(gw, gh);
        for y in 0..gh as i64 {
            for x in 0..gw as i64 {
                if on.v[y as usize * gw + x as usize] == 0 {
                    continue;
                }
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (xx, yy) = (x + dx, y + dy);
                        if xx >= 0 && yy >= 0 && (xx as usize) < gw && (yy as usize) < gh {
                            g.v[yy as usize * gw + xx as usize] = 1;
                        }
                    }
                }
            }
        }
        g
    };
    let (lab, sizes) = segment::components(&grown);
    let mut weight = vec![0usize; sizes.len()];
    for (i, &c) in cells.iter().enumerate() {
        let cx = ((i % gw) * b) as f64 * pt_per_px;
        let cy = ((i / gw) * b) as f64 * pt_per_px;
        if cx >= region[0] && cx < region[2] && cy >= region[1] && cy < region[3] {
            weight[lab[i] as usize] += c;
        }
    }
    let Some(best) = (1..weight.len()).max_by_key(|&i| weight[i]) else {
        return Vec::new();
    };
    let mut total = vec![0usize; sizes.len()];
    let mut bbox = vec![[usize::MAX, usize::MAX, 0, 0]; sizes.len()];
    for (i, &c) in cells.iter().enumerate() {
        if c > 0 {
            let g = lab[i] as usize;
            let (x, y) = (i % gw, i / gw);
            total[g] += c;
            let r = &mut bbox[g];
            r[0] = r[0].min(x * b);
            r[1] = r[1].min(y * b);
            r[2] = r[2].max(((x + 1) * b).min(kept.w));
            r[3] = r[3].max(((y + 1) * b).min(kept.h));
        }
    }
    let mut others: Vec<usize> = (1..sizes.len())
        .filter(|&g| g != best && total[g] as f64 >= OTHER_MAP_MIN_SHARE * total[best] as f64)
        .collect();
    others.sort_by(|a, b| total[*b].cmp(&total[*a]).then(a.cmp(b)));
    std::iter::once(best)
        .chain(others)
        .map(|g| bbox[g])
        .filter(|r| r[2] > r[0])
        .collect()
}

/// Where to look for the map: the largest rectangle of the page that meets none of the
/// panels (boxes reaching the page border, within 5 % of the page, and covering at least
/// 1 % of it). Candidate edges are the page and box edges.
pub fn search_region(page: PageRect, boxes: &[PageRect]) -> PageRect {
    let (pw, ph) = (page[2] - page[0], page[3] - page[1]);
    let panels: Vec<PageRect> = boxes
        .iter()
        .copied()
        .filter(|b| {
            let touches = b[0] <= page[0] + 0.05 * pw
                || b[1] <= page[1] + 0.05 * ph
                || b[2] >= page[2] - 0.05 * pw
                || b[3] >= page[3] - 0.05 * ph;
            touches && (b[2] - b[0]) * (b[3] - b[1]) >= 0.01 * pw * ph
        })
        .collect();
    let mut xs: Vec<f64> = vec![page[0], page[2]];
    let mut ys: Vec<f64> = vec![page[1], page[3]];
    for b in &panels {
        xs.extend([b[0], b[2]]);
        ys.extend([b[1], b[3]]);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut best = (0.0, page);
    for (i, &x0) in xs.iter().enumerate() {
        for &x1 in &xs[i + 1..] {
            for (j, &y0) in ys.iter().enumerate() {
                for &y1 in &ys[j + 1..] {
                    let area = (x1 - x0) * (y1 - y0);
                    if area <= best.0 {
                        continue;
                    }
                    let hit = panels
                        .iter()
                        .any(|b| b[0] < x1 && b[2] > x0 && b[1] < y1 && b[3] > y0);
                    if !hit {
                        best = (area, [x0, y0, x1, y1]);
                    }
                }
            }
        }
    }
    best.1
}

/// Map area around the drawn network `n`: grown by 10 % of its larger side (labels of the
/// places around the pistes; to the page edge when less than that is left), but not across
/// a side panel. On each side, a panel is a
/// box reaching the page border (within 5 % of the page) whose inner edge lies in the
/// outer fifth of the network or beyond; when such boxes cover at least 30 % of the
/// network's extent along that side, the map area stops at their innermost edge.
pub fn map_area(n: PageRect, page: PageRect, boxes: &[PageRect]) -> PageRect {
    let m = 0.1 * (n[2] - n[0]).max(n[3] - n[1]);
    // A map ending less than the margin short of the page edge goes to the edge (full
    // bleed: place names along the border).
    let lo = |v: f64, edge: f64| if v - m - edge < m { edge } else { v - m };
    let hi = |v: f64, edge: f64| if edge - v - m < m { edge } else { v + m };
    let mut a = [
        lo(n[0], page[0]),
        lo(n[1], page[1]),
        hi(n[2], page[2]),
        hi(n[3], page[3]),
    ];
    let span = |lo: f64, hi: f64, b0: f64, b1: f64| (hi.min(b1) - lo.max(b0)).max(0.0);
    let (nw, nh) = (n[2] - n[0], n[3] - n[1]);
    let (pw, ph) = (page[2] - page[0], page[3] - page[1]);
    for side in 0..4 {
        // (overlap along the side, inner edge)
        let beyond: Vec<(f64, f64)> = boxes
            .iter()
            .filter_map(|b| {
                let (out, ov, edge) = match side {
                    0 => (
                        b[0] <= page[0] + 0.05 * pw && b[2] <= n[0] + 0.2 * nw,
                        span(n[1], n[3], b[1], b[3]),
                        b[2],
                    ),
                    1 => (
                        b[1] <= page[1] + 0.05 * ph && b[3] <= n[1] + 0.2 * nh,
                        span(n[0], n[2], b[0], b[2]),
                        b[3],
                    ),
                    2 => (
                        b[2] >= page[2] - 0.05 * pw && b[0] >= n[2] - 0.2 * nw,
                        span(n[1], n[3], b[1], b[3]),
                        b[0],
                    ),
                    _ => (
                        b[3] >= page[3] - 0.05 * ph && b[1] >= n[3] - 0.2 * nh,
                        span(n[0], n[2], b[0], b[2]),
                        b[1],
                    ),
                };
                (out && ov > 0.0).then_some((ov, edge))
            })
            .collect();
        let extent = if side % 2 == 0 {
            n[3] - n[1]
        } else {
            n[2] - n[0]
        };
        if beyond.iter().map(|b| b.0).sum::<f64>() < 0.3 * extent {
            continue;
        }
        match side {
            0 => a[0] = beyond.iter().map(|b| b.1).fold(a[0], f64::max),
            1 => a[1] = beyond.iter().map(|b| b.1).fold(a[1], f64::max),
            2 => a[2] = beyond.iter().map(|b| b.1).fold(a[2], f64::min),
            _ => a[3] = beyond.iter().map(|b| b.1).fold(a[3], f64::min),
        }
    }
    a
}

/// Background colours of the map: the most frequent colours of the region (4-bit bins
/// per channel, mean colour of each bin) until they cover 80 % of its pixels, twelve at
/// most, as CIELAB.
pub fn map_backgrounds(img: &Rgb, region: PageRect, pt_per_px: f64) -> Vec<[f32; 3]> {
    let to_px = |v: f64| ((v / pt_per_px).max(0.0) as usize).min(img.w.max(img.h));
    let (x0, y0, x1, y1) = (
        to_px(region[0]),
        to_px(region[1]),
        to_px(region[2]).min(img.w),
        to_px(region[3]).min(img.h),
    );
    let mut bins: std::collections::HashMap<[u8; 3], (usize, [u64; 3])> = Default::default();
    let mut n = 0usize;
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            let p = img.get(x, y);
            let e = bins.entry([p[0] >> 4, p[1] >> 4, p[2] >> 4]).or_default();
            e.0 += 1;
            for (s, v) in e.1.iter_mut().zip(p) {
                *s += v as u64;
            }
            n += 1;
        }
    }
    let mut v: Vec<(usize, [u64; 3])> = bins.into_values().collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out = Vec::new();
    let mut acc = 0usize;
    for (count, sum) in v.into_iter().take(12) {
        if acc as f64 >= 0.8 * n as f64 {
            break;
        }
        acc += count;
        let c = count as u64;
        out.push(lab([
            (sum[0] / c) as u8,
            (sum[1] / c) as u8,
            (sum[2] / c) as u8,
        ]));
    }
    out
}

/// Line parameters for lines of median width `w` pixels (width = pixels / skeleton
/// length of a line component), calibrated on the two hand-written sheets.
pub fn line_params(w: f64, pt_per_px: f64) -> LineParams {
    LineParams {
        close_r: ((0.5 * w).round() as i64).clamp(1, 3),
        min_len_px: (22.0 * w).max(40.0),
        max_width_px: (4.3 * w).max(3.0 / pt_per_px),
        min_width_px: 0.85 * w,
        max_hole_px: (11.0 * w * w).round() as usize,
        spur_px: 3.2 * w,
    }
}

/// Provisional line parameters at a resolution, before the width is measured: lines of
/// 2 pt (references, network).
pub fn provisional_line_params(pt_per_px: f64) -> LineParams {
    line_params(2.0 / pt_per_px, pt_per_px)
}

/// Width (pixels) of the drawn lines: for each ink, the most common width of its
/// components that are long (20 pt of skeleton) and at most 4 pt wide (width = pixels /
/// skeleton length; histogram in half-pixel bins weighted by length, smoothed over three
/// bins: thick symbols drawn in an ink's colour are fewer than its lines); then the median
/// over the inks drawing at least 200 pt of such lines (the black ink also takes the text,
/// thinner). Returns the width and the per-ink widths.
pub fn measured_width(
    classes: &[u8],
    w: usize,
    h: usize,
    n_inks: usize,
    pt_per_px: f64,
) -> (f64, Vec<f64>) {
    let per: Vec<(f64, f64)> = (0..n_inks)
        .into_par_iter()
        .map(|ink| ink_width(classes, w, h, ink, pt_per_px))
        .collect();
    let mut ok: Vec<f64> = per
        .iter()
        .filter(|(_, l)| *l * pt_per_px >= 200.0)
        .map(|(wd, _)| *wd)
        .collect();
    ok.sort_by(f64::total_cmp);
    let med = if ok.is_empty() { 0.0 } else { ok[ok.len() / 2] };
    (med, per.into_iter().map(|p| p.0).collect())
}

/// Most common width of one ink's line components and their total skeleton length,
/// pixels.
fn ink_width(classes: &[u8], w: usize, h: usize, ink: usize, pt_per_px: f64) -> (f64, f64) {
    let min_len = 20.0 / pt_per_px;
    let max_w = 4.0 / pt_per_px;
    let mut hist = vec![0.0f64; (2.0 * max_w).ceil() as usize + 2];
    {
        let mask = segment::ink_mask(classes, w, h, ink as u8).close(1);
        let skel = crate::skeleton::thin(&mask);
        let (comp, sizes) = segment::components(&mask);
        let mut len = vec![0usize; sizes.len()];
        for (i, &s) in skel.v.iter().enumerate() {
            if s != 0 {
                len[comp[i] as usize] += 1;
            }
        }
        for (c, (&area, &l)) in sizes.iter().zip(&len).enumerate().skip(1) {
            let width = area as f64 / l.max(1) as f64;
            if (l as f64) < min_len || width > max_w || c == 0 {
                continue;
            }
            hist[(2.0 * width) as usize] += l as f64;
        }
    }
    let smooth = |i: usize| {
        hist[i]
            + hist.get(i + 1).copied().unwrap_or(0.0)
            + i.checked_sub(1).map_or(0.0, |j| hist[j])
    };
    let total: f64 = hist.iter().sum();
    let mode = (0..hist.len())
        .max_by(|&a, &b| smooth(a).total_cmp(&smooth(b)))
        .filter(|&i| smooth(i) > 0.0)
        .map_or(0.0, |i| (i as f64 + 0.5) / 2.0);
    (mode, total)
}

/// What the detection found, for the report (`autospec.json`).
#[derive(Debug, Clone, Serialize)]
pub struct AutoReport {
    /// PDF probe.
    pub probe: Probe,
    /// Legend lines found.
    pub entries: Vec<EntryCand>,
    /// Inks with their origin.
    pub inks: Vec<InkFound>,
    /// Median line width, pixels.
    pub line_width_px: f64,
    /// Width of the lines of each ink, pixels.
    pub ink_widths_px: Vec<f64>,
    /// Bounding box of the drawn network, page points.
    pub network: Option<PageRect>,
    /// Bounding boxes of the other drawn networks (other maps of the page), page points.
    pub other_networks: Vec<PageRect>,
    /// Region searched for the map (away from the side panels), page points.
    pub region: PageRect,
    /// Flat boxes found (masks and map frames).
    pub boxes: Vec<PageRect>,
}

/// Detected sheet and report.
#[derive(Debug, Clone, Serialize)]
pub struct AutoSpec {
    /// The sheet.
    pub spec: PlanSpec,
    /// How it was found.
    pub report: AutoReport,
    /// Map area and masks of every other map of the page ([`networks`]), tried in turn
    /// when the main map does not register.
    pub other_maps: Vec<(PageRect, Vec<PageRect>)>,
    /// Map registered: 0 for the main map, `i` for `other_maps[i - 1]`.
    pub map_index: usize,
}

/// Most frequent colour of the page (4-bit bins per channel), the background.
pub fn page_background(img: &Rgb) -> [u8; 3] {
    let mut freq: std::collections::HashMap<[u8; 3], usize> = Default::default();
    for p in img.px.as_chunks::<3>().0.iter().step_by(7) {
        *freq.entry([p[0] >> 4, p[1] >> 4, p[2] >> 4]).or_default() += 1;
    }
    let mode = freq
        .into_iter()
        .max_by_key(|(k, n)| (*n, *k))
        .map(|(k, _)| k)
        .unwrap_or([15, 15, 15]);
    [mode[0] * 16 + 8, mode[1] * 16 + 8, mode[2] * 16 + 8]
}

/// Contrast of every pixel with its surroundings: CIELAB distance to the mean colour of
/// the square of side `2r + 1` around it (a thin line differs from that mean, the inside
/// of a filled area does not).
pub fn local_contrast(img: &Rgb, r: usize) -> Vec<f32> {
    let (w, h) = (img.w, img.h);
    // Integral images of the three channels.
    let mut sat = vec![[0u64; 3]; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = [0u64; 3];
        for x in 0..w {
            let p = img.get(x, y);
            for c in 0..3 {
                row[c] += p[c] as u64;
                sat[(y + 1) * (w + 1) + x + 1][c] = sat[y * (w + 1) + x + 1][c] + row[c];
            }
        }
    }
    let mut cache: std::collections::HashMap<[u8; 3], [f32; 3]> = Default::default();
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let n = ((x1 - x0) * (y1 - y0)) as u64;
            let mut mean = [0u8; 3];
            for (c, m) in mean.iter_mut().enumerate() {
                let s = sat[y1 * (w + 1) + x1][c] + sat[y0 * (w + 1) + x0][c]
                    - sat[y0 * (w + 1) + x1][c]
                    - sat[y1 * (w + 1) + x0][c];
                *m = (s / n) as u8;
            }
            let p = img.get(x, y);
            let lp = *cache.entry(p).or_insert_with(|| lab(p));
            out[y * w + x] = de(lp, lab(mean));
        }
    }
    out
}

/// Reference colour of an ink on the map lines, from its legend colour (a thin legend
/// dash, blurred on a picture plan, is lighter than the lines): the map pixels of the same
/// colour name within ΔE 40 of the legend colour that stand out of their surroundings
/// (contrast ≥ 20), the fifth of highest contrast (line centres), median. `None` below
/// 200 pixels. Returns the colour and the pixels counted.
fn map_ref(
    img: &Rgb,
    contrast: &[f32],
    on_map: &dyn Fn(usize, usize) -> bool,
    legend: [u8; 3],
    name: &str,
) -> (Option<[u8; 3]>, usize) {
    let l0 = lab(legend);
    let mut cache: std::collections::HashMap<[u8; 3], bool> = Default::default();
    let mut px: Vec<(f32, [u8; 3])> = Vec::new();
    for y in 0..img.h {
        for x in 0..img.w {
            let i = y * img.w + x;
            if contrast[i] < 20.0 || !on_map(x, y) {
                continue;
            }
            let p = img.get(x, y);
            let ok = *cache.entry(p).or_insert_with(|| {
                let named = colour_name(p);
                named == Some(name) && de(lab(p), l0) <= 40.0
            });
            if ok {
                px.push((contrast[i], p));
            }
        }
    }
    if px.len() < 200 {
        return (None, px.len());
    }
    px.sort_by(|a, b| b.0.total_cmp(&a.0));
    let top: Vec<[u8; 3]> = px[..px.len().div_ceil(5)].iter().map(|c| c.1).collect();
    (Some(median_rgb(&top)), px.len())
}

/// Detects the reading sheet of a plan. `lines`: text lines (text layer, or raw OCR of a
/// picture plan); `extent`: geographic extent to query (refined after a first
/// registration by the caller).
pub fn detect(
    src: &PlanSource,
    probe: &Probe,
    img: &Rgb,
    pt_per_px: f64,
    lines: &[TextBox],
    extent: BBox,
) -> AutoSpec {
    let k = pt_per_px;
    let labels = label_source(probe);
    let dpi = (72.0 / k).round() as u32;
    let page = [0.0, 0.0, img.w as f64 * k, img.h as f64 * k];
    // Legend lines and their dash colours.
    let mut entries = entry_candidates(lines, labels);
    for e in &mut entries {
        e.dash = marker_colour(img, e.rect, k, lines);
    }
    let mut inks = legend_inks(&entries);
    // Flat boxes and the region where the map is (away from the side panels).
    let boxes = flat_boxes(img, k);
    let region = search_region(page, &boxes);
    let inside =
        |b: &PageRect, a: &PageRect| b[0] < a[2] && b[2] > a[0] && b[1] < a[3] && b[3] > a[1];
    // References on the map lines (in the region, off the legend lines themselves).
    let prov = provisional_line_params(k);
    let rects: Vec<PageRect> = entries.iter().map(|e| e.rect).collect();
    let on_region = |x: usize, y: usize| {
        let (px, py) = ((x as f64 + 0.5) * k, (y as f64 + 0.5) * k);
        px >= region[0]
            && px < region[2]
            && py >= region[1]
            && py < region[3]
            && !rects
                .iter()
                .any(|r| px >= r[0] - 20.0 && px < r[2] && py >= r[1] && py < r[3])
    };
    // Thresholds: half the distance to the nearest background colour of the map (those
    // more than ΔE 10 from every ink), within 8 to 22. Between two inks the nearest wins
    // (segment::classify): their distance does not limit the threshold.
    let backgrounds = map_backgrounds(img, region, k);
    let thresholds = |inks: &mut Vec<InkFound>| {
        let refs: Vec<[f32; 3]> = inks
            .iter()
            .flat_map(|f| f.ink.refs.iter().map(|&c| lab(c)))
            .collect();
        let bgs: Vec<[f32; 3]> = backgrounds
            .iter()
            .copied()
            .filter(|b| refs.iter().all(|r| de(*b, *r) > 10.0))
            .collect();
        for f in inks.iter_mut() {
            let near = f
                .ink
                .refs
                .iter()
                .flat_map(|&c| bgs.iter().map(move |r| de(lab(c), *r)))
                .fold(f32::INFINITY, f32::min);
            f.ink.max_de = (0.5 * near).clamp(8.0, 22.0);
        }
    };
    // A legend that gives fewer than two piste lines a difficulty ink (no coloured dash, no
    // colour word: pictograms, numbers, a table) leaves the drawing's own line colours.
    let contrast = local_contrast(img, (3.0 / k).round() as usize);
    let legend_ski: usize = inks
        .iter()
        .filter(|f| f.ink.difficulty.is_some())
        .map(|f| f.entries)
        .sum();
    let from_drawing = legend_ski < 2;
    if from_drawing {
        for f in drawing_inks(img, &contrast, &on_region, &backgrounds) {
            if !inks.iter().any(|i| i.ink.name == f.ink.name) {
                inks.push(f);
            }
        }
    }
    thresholds(&mut inks);
    let mut spec = PlanSpec::new(
        src,
        dpi,
        region,
        Vec::new(),
        extent,
        inks.iter().map(|f| f.ink.clone()).collect(),
        labels,
        prov,
        false,
        Vec::new(),
    );
    // Inks read from the drawing must draw lines: at least [`DRAWING_MIN_LINE_PT`] of
    // thin line components (a texture or a filled area of the same colour does not).
    if from_drawing {
        let classes = segment::classify(&spec, img, k);
        let lens: Vec<f64> = (0..spec.inks.len())
            .into_par_iter()
            .map(|i| ink_width(&classes, img.w, img.h, i, k).1 * k)
            .collect();
        let mut i = 0;
        inks.retain(|f| {
            let keep = f.source != InkSource::Drawing || lens[i] >= DRAWING_MIN_LINE_PT;
            i += 1;
            keep
        });
        thresholds(&mut inks);
        spec.inks = inks.iter().map(|f| f.ink.clone()).collect();
    }
    // Boxes of the region holding a quarter of the drawn lines or more are the flat
    // background of the map itself; the others hide the map (masks).
    let kept_of = |spec: &PlanSpec| {
        let classes = segment::classify(spec, img, k);
        let kepts: Vec<Mask> = (0..spec.inks.len())
            .into_par_iter()
            .map(|ink| segment::ink_lines(&classes, img.w, img.h, ink as u8, &spec.line).0)
            .collect();
        let mut kept_all = Mask::new(img.w, img.h);
        for kept in &kepts {
            kept_all.or(kept);
        }
        (classes, kept_all)
    };
    let count_in = |m: &Mask, b: &PageRect, shrink: f64| {
        let to_px = |v: f64| ((v / k).max(0.0) as usize).min(img.w.max(img.h));
        let (x0, y0, x1, y1) = (
            to_px(b[0] + shrink),
            to_px(b[1] + shrink),
            to_px(b[2] - shrink).min(img.w),
            to_px(b[3] - shrink).min(img.h),
        );
        let mut n = 0usize;
        for y in y0..y1 {
            for x in x0..x1 {
                n += m.v[y * img.w + x] as usize;
            }
        }
        n
    };
    let (classes0, kept0) = kept_of(&spec);
    let total0 = kept0.count().max(1) as f64;
    let frames: Vec<PageRect> = boxes
        .iter()
        .copied()
        .filter(|b| count_in(&kept0, b, 4.0) as f64 >= 0.25 * total0)
        .collect();
    let hiding = |a: &PageRect| -> Vec<PageRect> {
        boxes
            .iter()
            .copied()
            .filter(|b| inside(b, a) && !frames.contains(b))
            .collect()
    };
    spec.masks = hiding(&region);
    // Line width and parameters (inks at their legend colours).
    let (width, ink_widths) = measured_width(&classes0, img.w, img.h, spec.inks.len(), k);
    if width > 0.0 {
        spec.line = line_params(width, k);
    }
    // References on the map lines (in the region, off the masks and the legend lines,
    // within 2 px of a line drawn near the legend colours, ΔE 22: not a logo or a filled
    // area).
    let masks = spec.masks.clone();
    let loose = {
        let mut s = spec.clone();
        for i in &mut s.inks {
            i.max_de = 22.0;
        }
        s
    };
    let (_, kept1) = kept_of(&loose);
    let near_line = {
        let mut m = Mask::new(img.w, img.h);
        for y in 0..img.h {
            for x in 0..img.w {
                if kept1.v[y * img.w + x] == 0 {
                    continue;
                }
                for yy in y.saturating_sub(2)..(y + 3).min(img.h) {
                    for xx in x.saturating_sub(2)..(x + 3).min(img.w) {
                        m.v[yy * img.w + xx] = 1;
                    }
                }
            }
        }
        m
    };
    let on_map = |x: usize, y: usize| {
        let (px, py) = ((x as f64 + 0.5) * k, (y as f64 + 0.5) * k);
        near_line.v[y * img.w + x] != 0
            && on_region(x, y)
            && !masks
                .iter()
                .any(|b| px >= b[0] && px < b[2] && py >= b[1] && py < b[3])
    };
    for f in &mut inks {
        let mut refs = Vec::new();
        for &c in &f.legend_colours {
            let (r, n) = map_ref(img, &contrast, &on_map, c, &f.ink.name);
            f.map_pixels += n;
            // The line is the dash unblurred: as dark or darker, same hue (within 15°).
            let hue = |l: [f32; 3]| l[2].atan2(l[1]).to_degrees();
            let same = |r: [u8; 3]| {
                let (a, b) = (lab(r), lab(c));
                let dh = (hue(a) - hue(b)).rem_euclid(360.0);
                a[0] <= b[0] + 2.0
                    && (a[1].hypot(a[2]) < 12.0
                        || b[1].hypot(b[2]) < 12.0
                        || dh.min(360.0 - dh) <= 15.0)
            };
            refs.push(r.filter(|r| same(*r)).unwrap_or(c));
        }
        f.ink.refs = refs;
    }
    // Lines of a colour the piste legend does not list (walkers' or snowshoe paths):
    // without an ink of their own they would be read as the nearest piste ink. A colour
    // name drawing at least a quarter of the median line pixels of the legend inks becomes
    // an ink of its own, of non-ski kind (followed on non-nordic OSM ways only), with no
    // legend entry: never transcribed.
    let mut by_name: BTreeMap<&'static str, Vec<(f32, [u8; 3])>> = BTreeMap::new();
    for y in 0..img.h {
        for x in 0..img.w {
            let i = y * img.w + x;
            if contrast[i] < 20.0 || !on_map(x, y) {
                continue;
            }
            let p = img.get(x, y);
            if let Some(n) = colour_name(p) {
                by_name.entry(n).or_default().push((contrast[i], p));
            }
        }
    }
    let mut counts: Vec<usize> = inks
        .iter()
        .map(|f| by_name.get(f.ink.name.as_str()).map_or(0, Vec::len))
        .collect();
    counts.sort_unstable();
    let median = counts.get(counts.len() / 2).copied().unwrap_or(0);
    for (name, mut px) in by_name {
        if inks.iter().any(|f| f.ink.name == name) || 4 * px.len() < median || px.len() < 200 {
            continue;
        }
        px.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top: Vec<[u8; 3]> = px[..px.len().div_ceil(5)].iter().map(|c| c.1).collect();
        let r = median_rgb(&top);
        inks.push(InkFound {
            ink: Ink {
                name: name.to_owned(),
                refs: vec![r],
                max_de: 0.0,
                difficulty: None,
                kind: PisteKind::Pedestrian,
            },
            legend_colours: Vec::new(),
            source: InkSource::Unlisted,
            entries: 0,
            map_pixels: px.len(),
        });
    }
    thresholds(&mut inks);
    spec.inks = inks.iter().map(|f| f.ink.clone()).collect();
    // Drawn network (kept line pixels of every ink in the region, off the masks), main
    // group, and the map area around it.
    // On the whole page (the region may stop short of the map, at an advertisement in
    // its frame), off every box but the map's own background.
    let mut whole = spec.clone();
    whole.map_area = page;
    whole.masks = boxes
        .iter()
        .copied()
        .filter(|b| !frames.contains(b))
        .collect();
    let (_, kept_net) = kept_of(&whole);
    let nets: Vec<PageRect> = networks(&kept_net, k, region)
        .into_iter()
        .map(|r| px_rect(r, k))
        .collect();
    let network = nets.first().copied();
    // Other maps of the page: each its own area around its network (not grown over the
    // main map), its own masks.
    let main_area = network.map(|n| map_area(n, page, &boxes));
    let other_maps: Vec<(PageRect, Vec<PageRect>)> = nets
        .iter()
        .skip(1)
        .map(|n| {
            let mut a = map_area(*n, page, &boxes);
            if let Some(m) = main_area {
                // Shrink towards the network on the sides where it meets the main map.
                if inside(&a, &m) {
                    if n[0] >= m[2] {
                        a[0] = a[0].max(m[2]);
                    }
                    if n[2] <= m[0] {
                        a[2] = a[2].min(m[0]);
                    }
                    if n[1] >= m[3] {
                        a[1] = a[1].max(m[3]);
                    }
                    if n[3] <= m[1] {
                        a[3] = a[3].min(m[1]);
                    }
                }
            }
            (a, hiding(&a))
        })
        .collect();
    if let Some(n) = network {
        let a = map_area(n, page, &boxes);
        // The region free of panels is map too (the network may stop short of it).
        spec.map_area = [
            a[0].min(region[0]),
            a[1].min(region[1]),
            a[2].max(region[2]),
            a[3].max(region[3]),
        ];
    }
    spec.masks = hiding(&spec.map_area.clone());
    AutoSpec {
        spec,
        report: AutoReport {
            probe: probe.clone(),
            entries,
            inks,
            line_width_px: width,
            ink_widths_px: ink_widths,
            network,
            other_networks: nets.iter().skip(1).copied().collect(),
            region,
            boxes,
        },
        other_maps,
        map_index: 0,
    }
}

/// Margin of the extent around the drawn map, metres.
pub const EXTENT_MARGIN_M: f64 = 3000.0;

/// A plan read with an automatic sheet.
pub struct AutoRegistration {
    /// The sheet and how it was found (its extent: the extent drawn, once placed).
    pub auto: AutoSpec,
    /// The registration with that sheet.
    pub reg: Registration,
    /// Extent given as a hint (around the domains).
    pub hint: BBox,
}

/// Downloads and reads a plan: probe, rendering at [`choose_dpi`], its text lines (text
/// layer, else OCR of the raw rendering; both cached).
pub fn read_plan(
    src: &PlanSource,
    layout: &DataLayout,
    dl: &Downloader,
    engine: &ocrs::OcrEngine,
) -> Result<(Probe, input::PlanInput, Vec<TextBox>)> {
    let fetched = dl.fetch(&Request::get(&src.source, &src.url).max_age(input::PDF_MAX_AGE))?;
    let text = input::text_layer(&fetched, layout)?;
    let probe = input::probe(&fetched.path, &text)?;
    let dpi = choose_dpi(&probe);
    let inp = input::load_fetched(fetched, dpi, layout)?;
    let lines: Vec<TextBox> = match label_source(&probe) {
        LabelSource::TextLayer => inp.text.lines.iter().map(|(l, _)| l.clone()).collect(),
        LabelSource::Ocr => crate::build::ocr_page(engine, &inp, false, layout)?,
    };
    Ok((probe, inp, lines))
}

/// Census domains a plan the collector could not attach names
/// ([`crate::collect::locate`]: its text, title and file name).
pub fn locate_plan(
    src: &PlanSource,
    layout: &DataLayout,
    dl: &Downloader,
    engine: &ocrs::OcrEngine,
    census: &[crate::collect::Domain],
) -> Result<Vec<String>> {
    let (_, _, lines) = read_plan(src, layout, dl, engine)?;
    let texts: Vec<String> = lines.into_iter().map(|l| l.text).collect();
    let file = src.url.rsplit('/').next().unwrap_or_default().to_owned();
    Ok(crate::collect::locate(
        &texts,
        &[src.title.clone(), file],
        census,
    ))
}

/// Reads a plan from its source alone: probe, resolution, text or OCR, sheet detection,
/// registration (labels placed on the hint extent, then OSM pistes of the extent actually
/// drawn: map area carried onto the map, plus [`EXTENT_MARGIN_M`]).
pub fn register_auto(
    src: &PlanSource,
    hint: BBox,
    layout: &DataLayout,
    dl: &Downloader,
    planet: &Planet,
    engine: &ocrs::OcrEngine,
) -> Result<AutoRegistration> {
    let t = std::time::Instant::now();
    let (probe, inp, lines) = read_plan(src, layout, dl, engine)?;
    let mut auto = detect(src, &probe, &inp.image, inp.pt_per_px, &lines, hint);
    let load_s = t.elapsed().as_secs_f64();
    // The main map, then (page with several maps) each other map in turn: the first that
    // registers. Only a failure to place the plan moves on to the next map.
    let candidates: Vec<(PageRect, Vec<PageRect>)> =
        std::iter::once((auto.spec.map_area, auto.spec.masks.clone()))
            .chain(auto.other_maps.iter().cloned())
            .collect();
    let mut last = None;
    for (i, (area, masks)) in candidates.into_iter().enumerate() {
        let mut spec = auto.spec.clone();
        spec.map_area = area;
        spec.masks = masks;
        match register_input(
            spec,
            inp.clone(),
            load_s,
            layout,
            dl,
            planet,
            // A text layer needs no OCR.
            (auto.spec.labels == LabelSource::Ocr).then_some(engine),
            Some(EXTENT_MARGIN_M),
        ) {
            Ok(reg) => {
                auto.spec = reg.spec.clone();
                auto.map_index = i;
                return Ok(AutoRegistration { auto, reg, hint });
            }
            Err(e @ crate::Error::Fit(_)) => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| crate::Error::Fit(format!("{}: no map found", src.id))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PdfImage;

    fn probe(words: usize, page: [f64; 2], images: Vec<PdfImage>) -> Probe {
        Probe {
            pages: 1,
            page_pt: page,
            images,
            words,
            created: None,
            creator: None,
        }
    }

    #[test]
    fn resolution_and_label_source_follow_the_pdf() {
        // Cervières: one 1280 px image at 155 ppi over a 595 pt page, no text.
        let cerv = probe(
            0,
            [595.5, 595.5],
            vec![PdfImage {
                w: 1280,
                h: 1255,
                ppi: 155.0,
            }],
        );
        assert!(cerv.picture_only());
        assert_eq!(choose_dpi(&cerv), 300);
        assert_eq!(label_source(&cerv), LabelSource::Ocr);
        // Chapelle-des-Bois: vector plan of 1417 × 1191 pt with a text layer.
        let enj = probe(1827, [1417.32, 1190.55], Vec::new());
        assert!(!enj.picture_only());
        assert_eq!(choose_dpi(&enj), 200);
        assert_eq!(label_source(&enj), LabelSource::TextLayer);
    }

    #[test]
    fn colours_are_named_by_hue() {
        for (c, n) in [
            ([57, 164, 69], Some("green")),
            ([163, 204, 93], Some("green")),
            ([4, 131, 196], Some("blue")),
            ([226, 6, 28], Some("red")),
            ([140, 45, 52], Some("red")),
            ([238, 115, 44], Some("orange")),
            ([205, 172, 76], Some("orange")),
            ([255, 237, 0], Some("yellow")),
            ([3, 3, 3], Some("black")),
            ([10, 42, 48], Some("black")),
            ([104, 72, 92], Some("purple")),
            ([128, 128, 128], None),
            ([255, 255, 255], None),
        ] {
            assert_eq!(colour_name(c), n, "{c:?}");
        }
    }

    #[test]
    fn legend_lines_give_inks_kinds_and_two_reds() {
        let e = |text: &str, word: Option<&'static str>, dash: [u8; 3]| EntryCand {
            text: text.into(),
            rect: [0.0; 4],
            word_ink: word,
            kind: kind_words(text),
            dash: Some(dash),
        };
        let inks = legend_inks(&[
            e("Chalet Pin 11,0 km", None, [226, 6, 28]),
            e("Combe David 7,2 km", None, [225, 8, 30]),
            e("Les Isles 3,1 km", None, [54, 165, 54]),
            e("Liaison Mouthe 4,4 km", None, [238, 115, 44]),
            e("Piste rouge : le Cancel 3,5 km", Some("red"), [174, 45, 55]),
            e(
                "Piste rouge : Le Chemin du Sioux",
                Some("red"),
                [216, 148, 148],
            ),
        ]);
        let by = |n: &str| inks.iter().find(|i| i.ink.name == n).unwrap();
        assert_eq!(by("red").entries, 4);
        assert_eq!(by("red").ink.difficulty.as_deref(), Some("red"));
        assert_eq!(by("green").ink.kind, PisteKind::Ski);
        assert_eq!(by("orange").ink.kind, PisteKind::Liaison);
        assert_eq!(by("orange").ink.difficulty, None);
        // The word « rouge » groups two dash colours: two references.
        let reds: Vec<_> = inks.iter().filter(|i| i.ink.name == "red").collect();
        assert_eq!(reds.len(), 1);
        assert_eq!(
            inks.iter().map(|i| i.ink.refs.len()).collect::<Vec<_>>(),
            vec![3, 1, 1]
        );
        assert_eq!(
            kind_words("Jumbo (Multi-activités) 11,0 km"),
            Some(PisteKind::MultiActivity)
        );
        assert_eq!(
            colour_word_ink("Piste de liaison Cervières"),
            Some("orange")
        );
        assert_eq!(colour_word_ink("Combe Verte 8,4 km"), None);
    }

    #[test]
    fn drawing_inks_are_the_line_colours_off_the_background() {
        // Pale blue background, a red and a dark blue line, a pale green filled area (no
        // contrast inside), a long black line (left out: roads, contours).
        let (w, h) = (200, 120);
        let mut img = Rgb::new(w, h, [200, 225, 240]);
        for x in 10..190 {
            for y in 20..23 {
                img.put(x, y, [220, 30, 40]);
            }
            for y in 60..63 {
                img.put(x, y, [20, 100, 190]);
            }
            for y in 100..103 {
                img.put(x, y, [10, 10, 10]);
            }
        }
        for y in 70..95 {
            for x in 20..120 {
                img.put(x, y, [170, 230, 170]);
            }
        }
        let contrast = local_contrast(&img, 3);
        let bgs = map_backgrounds(&img, [0.0, 0.0, w as f64, h as f64], 1.0);
        let inks = drawing_inks(&img, &contrast, &|_, _| true, &bgs);
        let names: Vec<&str> = inks.iter().map(|f| f.ink.name.as_str()).collect();
        assert_eq!(names, vec!["blue", "red"], "{inks:?}");
        assert!(inks.iter().all(|f| f.source == InkSource::Drawing
            && f.ink.kind == PisteKind::Ski
            && f.ink.difficulty.as_deref() == Some(f.ink.name.as_str())));
        let blue = &inks[0].ink.refs;
        assert_eq!(blue.len(), 1, "{blue:?}");
        assert!(de(lab(blue[0]), lab([20, 100, 190])) < 10.0, "{blue:?}");
        // The background itself is never an ink, however textured.
        assert!(
            drawing_inks(&img, &vec![50.0; w * h], &|_, y| y < 15, &bgs).is_empty(),
            "background read as an ink"
        );
    }

    #[test]
    fn dash_colour_is_the_core_of_the_dash() {
        // White band with a 10 × 3 px red dash and anti-aliased rows around it.
        let mut img = Rgb::new(60, 20, [255, 255, 255]);
        for x in 10..20 {
            img.put(x, 8, [240, 160, 160]);
            for y in 9..12 {
                img.put(x, y, [200, 20, 30]);
            }
            img.put(x, 12, [240, 160, 160]);
        }
        // Text rect from x = 24 px (1 pt per px).
        let c = dash_colour(&img, [24.0, 5.0, 50.0, 15.0], 1.0).unwrap();
        assert_eq!(c, [200, 20, 30]);
    }

    #[test]
    fn flat_boxes_find_panels_with_text_and_ignore_the_map() {
        let (w, h) = (300, 200);
        let mut img = Rgb::new(w, h, [200, 230, 210]);
        // Textured map: a gradient (no flat region).
        for y in 0..h {
            for x in 0..200 {
                img.put(x as i64, y as i64, [(x % 251) as u8, (y % 241) as u8, 120]);
            }
        }
        // Panel on the right: white with dark text strokes.
        for y in 0..h {
            for x in 200..w {
                let text = (y % 20 < 4) && (x % 7 < 3) && x > 210 && x < 290;
                img.put(
                    x as i64,
                    y as i64,
                    if text { [20, 20, 20] } else { [255, 255, 255] },
                );
            }
        }
        let boxes = flat_boxes(&img, 0.5);
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        let b = boxes[0];
        assert!(
            (b[0] - 100.0).abs() < 1.0 && (b[2] - 150.0).abs() < 1.0,
            "{b:?}"
        );
        // The largest rectangle away from the panel is the map.
        let page = [0.0, 0.0, 150.0, 100.0];
        assert_eq!(search_region(page, &boxes), [0.0, 0.0, 100.0, 100.0]);
    }

    #[test]
    fn map_area_grows_around_the_network_and_stops_at_panels() {
        let page = [0.0, 0.0, 1000.0, 800.0];
        // Network in the middle, a left panel touching the page border.
        let n = [300.0, 200.0, 800.0, 600.0];
        let panel = [0.0, 0.0, 250.0, 800.0];
        let a = map_area(n, page, &[panel]);
        assert_eq!(a, [250.0, 150.0, 850.0, 650.0]);
        // Close to the page edge: snapped to it.
        let a = map_area([300.0, 60.0, 800.0, 600.0], page, &[]);
        assert_eq!(a[1], 0.0);
    }

    #[test]
    fn line_parameters_scale_with_the_width() {
        let p = line_params(3.75, 0.36);
        assert_eq!(p.close_r, 2);
        assert!((p.max_width_px - 16.1).abs() < 0.1);
        assert!((p.min_width_px - 3.19).abs() < 0.01);
        let p = line_params(2.25, 0.24);
        assert_eq!(p.close_r, 1);
        assert!((p.min_width_px - 1.91).abs() < 0.01);
    }
}
