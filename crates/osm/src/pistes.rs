//! Nordic trails (P2, #15): classification of OSM ways and relations, attributes taken from
//! the relations.
//!
//! - A **piste relation** is a relation whose `piste:type` contains `nordic`, `hike` or
//!   `snowshoe`, or a `route=ski` relation without `piste:type` (nordic by definition).
//! - A **piste way** is a way whose own `piste:type` contains one of these types, or a
//!   member of a piste relation (many road segments of a trail carry no piste tag: the
//!   relation alone says they are groomed). Areas (`area=yes`) are skipped.
//! - Name and colour come from the relations. Way-level `piste:difficulty` is inconsistent
//!   (report 02: shared segments under the black « Greffier » are tagged `easy`), so the
//!   difficulty of a segment comes first from the colour of its relations, then from their
//!   `piste:difficulty`, and only then from the way.
//! - A segment shared by several relations gets `difficulty` = the **easiest** of them (a
//!   green loop is skiable end to end at green level, so every segment of it is) and
//!   `difficulty_max` = the hardest.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;
use skimap_common::geom::{BBox, LonLat};

use crate::parse::{MemberKind, OsmData, Tags};

/// Winter use of a trail kept by this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Use {
    /// Cross-country skiing (`piste:type=nordic`).
    Nordic,
    /// Winter hiking and snowshoeing on a marked trail (`piste:type=hike`).
    Hike,
    /// Snowshoe trail (`piste:type=snowshoe`).
    Snowshoe,
}

impl Use {
    fn parse(v: &str) -> Option<Use> {
        match v.trim() {
            "nordic" => Some(Use::Nordic),
            "hike" => Some(Use::Hike),
            "snowshoe" => Some(Use::Snowshoe),
            _ => None,
        }
    }
}

fn uses_of(tags: &Tags) -> BTreeSet<Use> {
    tags.get("piste:type")
        .map(|v| v.split(';').filter_map(Use::parse).collect())
        .unwrap_or_default()
}

/// Piste colour, as used for nordic trails in France (verte, bleue, rouge, noire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    /// Verte.
    Green,
    /// Bleue.
    Blue,
    /// Rouge.
    Red,
    /// Noire.
    Black,
    /// Any other colour (orange, yellow, purple...): no difficulty implied.
    Other,
}

impl Colour {
    /// Parses an OSM `colour` value: CSS-like names (English and French) or `#rgb` /
    /// `#rrggbb`. Hex colours are classified by hue; dark colours are black.
    pub fn parse(v: &str) -> Option<Colour> {
        let v = v.trim().to_ascii_lowercase();
        if v.is_empty() {
            return None;
        }
        let named = match v.as_str() {
            "green" | "vert" | "verte" | "darkgreen" | "lime" | "forestgreen" => {
                Some(Colour::Green)
            }
            "blue" | "bleu" | "bleue" | "navy" | "darkblue" | "royalblue" => Some(Colour::Blue),
            "red" | "rouge" | "darkred" | "crimson" => Some(Colour::Red),
            "black" | "noir" | "noire" => Some(Colour::Black),
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        match parse_hex(&v) {
            Some((r, g, b)) => Some(classify_rgb(r, g, b)),
            None => Some(Colour::Other),
        }
    }

    /// Difficulty implied by the colour (European convention for nordic trails).
    pub fn difficulty(self) -> Option<Difficulty> {
        match self {
            Colour::Green => Some(Difficulty::Novice),
            Colour::Blue => Some(Difficulty::Easy),
            Colour::Red => Some(Difficulty::Intermediate),
            Colour::Black => Some(Difficulty::Advanced),
            Colour::Other => None,
        }
    }
}

fn parse_hex(v: &str) -> Option<(f64, f64, f64)> {
    let h = v.strip_prefix('#')?;
    let digits: Vec<u8> = h
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    let (r, g, b) = match digits.as_slice() {
        [r, g, b] => (r * 17, g * 17, b * 17),
        [r1, r2, g1, g2, b1, b2] => (r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2),
        _ => return None,
    };
    Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0))
}

fn classify_rgb(r: f64, g: f64, b: f64) -> Colour {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 0.3 {
        return Colour::Black;
    }
    let delta = max - min;
    if delta / max < 0.35 {
        // Grey or white.
        return Colour::Other;
    }
    let hue = if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    match hue {
        h if !(20.0..340.0).contains(&h) => Colour::Red,
        h if (80.0..170.0).contains(&h) => Colour::Green,
        h if (180.0..265.0).contains(&h) => Colour::Blue,
        _ => Colour::Other,
    }
}

/// OSM `piste:difficulty`, in increasing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    /// Very easy (green in France).
    Novice,
    /// Easy (blue).
    Easy,
    /// Intermediate (red).
    Intermediate,
    /// Advanced (black).
    Advanced,
    /// Expert.
    Expert,
    /// Freeride / extreme.
    Freeride,
}

impl Difficulty {
    /// Parses an OSM `piste:difficulty` value.
    pub fn parse(v: &str) -> Option<Difficulty> {
        match v.trim() {
            "novice" => Some(Difficulty::Novice),
            "easy" => Some(Difficulty::Easy),
            "intermediate" => Some(Difficulty::Intermediate),
            "advanced" => Some(Difficulty::Advanced),
            "expert" => Some(Difficulty::Expert),
            "freeride" | "extreme" => Some(Difficulty::Freeride),
            _ => None,
        }
    }
}

/// Where the difficulty of a segment comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DifficultySource {
    /// `colour` of the relations.
    RelationColour,
    /// `piste:difficulty` of the relations.
    RelationDifficulty,
    /// `piste:difficulty` of the way (no usable relation value).
    Way,
}

/// Grooming of a segment, from `piste:grooming` (`;` or `+` separated).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Grooming {
    /// Raw value, from the way, else from the relations.
    pub raw: Option<String>,
    /// Classic tracks.
    pub classic: bool,
    /// Skating lane.
    pub skating: bool,
    /// Not groomed (`backcountry`, `no`, `none`): tracks made by skiers.
    pub backcountry: bool,
    /// Packed with a snowmobile (`scooter`).
    pub scooter: bool,
}

impl Grooming {
    /// Parses a `piste:grooming` value.
    pub fn parse(v: &str) -> Grooming {
        let mut g = Grooming {
            raw: Some(v.to_owned()),
            ..Default::default()
        };
        for p in v.split([';', '+', ',']).map(str::trim) {
            match p {
                "classic" => g.classic = true,
                "skating" | "skate" => g.skating = true,
                "backcountry" | "no" | "none" => g.backcountry = true,
                "scooter" => g.scooter = true,
                _ => {}
            }
        }
        g
    }
}

/// A piste relation (route).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PisteRelation {
    /// OSM relation id.
    pub id: i64,
    /// `piste:name`, else `name`.
    pub name: Option<String>,
    /// `piste:ref`, else `ref`.
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    /// Classified `colour`.
    pub colour: Option<Colour>,
    /// Raw `colour`.
    pub colour_raw: Option<String>,
    /// `piste:difficulty` of the relation.
    pub difficulty_tag: Option<Difficulty>,
    /// Difficulty implied by the colour, else by `piste:difficulty`.
    pub difficulty: Option<Difficulty>,
    /// Uses (`piste:type`).
    pub uses: BTreeSet<Use>,
    /// `piste:grooming` of the relation.
    #[serde(skip)]
    pub grooming: Option<String>,
    /// Member ways, in order, with their role.
    #[serde(skip)]
    pub ways: Vec<(i64, String)>,
}

/// A `site=piste` relation: the operator's own grouping of trails into a ski area.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteRelation {
    /// OSM relation id.
    pub id: i64,
    /// `name`.
    pub name: Option<String>,
    /// Ways reached directly or through member route relations (any depth). Ways of a
    /// nested `site=piste` relation belong to that inner site, not to this one.
    #[serde(skip)]
    pub ways: BTreeSet<i64>,
    /// Enclosing `site=piste` relations: those listing this one as a member, and those
    /// whose ways are a strict superset of this one's.
    pub parents: Vec<i64>,
}

/// One-way status of a segment (`piste:oneway`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Oneway {
    /// Tagged one-way in the way direction (`yes`, `1`, `true`).
    Forward,
    /// Tagged one-way against the way direction (`-1`, `reverse`).
    Backward,
    /// Tagged two-way (`no`).
    No,
    /// Not tagged: treated as two-way.
    Unknown,
}

impl Oneway {
    fn parse(v: Option<&str>) -> Oneway {
        match v.map(str::trim) {
            Some("yes" | "1" | "true") => Oneway::Forward,
            Some("-1" | "reverse") => Oneway::Backward,
            Some("no" | "0" | "false") => Oneway::No,
            _ => Oneway::Unknown,
        }
    }
}

/// A piste way with its derived attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Piste {
    /// OSM way id.
    pub way_id: i64,
    /// Uses: the way's own `piste:type`, plus those of its relations.
    pub uses: BTreeSet<Use>,
    /// Whether the way itself carries a kept `piste:type` (else: only through relations).
    pub tagged: bool,
    /// Way `piste:name`, else `name`.
    pub way_name: Option<String>,
    /// Indices of the piste relations containing the way (into [`PisteSet::relations`]).
    pub relations: Vec<usize>,
    /// Easiest difficulty among the sources of the highest-priority kind.
    pub difficulty: Option<Difficulty>,
    /// Hardest difficulty among the same sources.
    pub difficulty_max: Option<Difficulty>,
    /// Source of [`Piste::difficulty`].
    pub difficulty_source: Option<DifficultySource>,
    /// Way `piste:difficulty` (kept for comparison; inconsistent, see module doc).
    pub way_difficulty: Option<Difficulty>,
    /// Grooming.
    pub grooming: Grooming,
    /// One-way status.
    pub oneway: Oneway,
    /// `piste:lit` / `lit`.
    pub lit: Option<bool>,
    /// Kept node ids and positions (nodes outside the area of interest are skipped).
    pub points: Vec<(i64, LonLat)>,
    /// Length in metres (haversine).
    pub length_m: f64,
}

impl Piste {
    /// Whether the piste is (also) a nordic ski trail.
    pub fn is_nordic(&self) -> bool {
        self.uses.contains(&Use::Nordic)
    }

    /// Whether any point lies in `b`.
    pub fn touches(&self, b: &BBox) -> bool {
        self.points.iter().any(|(_, p)| b.contains(*p))
    }

    /// Mean latitude of the vertices (used to compare with research report 07).
    pub fn mean_lat(&self) -> f64 {
        self.points.iter().map(|(_, p)| p.lat).sum::<f64>() / self.points.len().max(1) as f64
    }
}

/// Length of a polyline in metres.
pub fn polyline_length_m(points: &[(i64, LonLat)]) -> f64 {
    points.windows(2).map(|w| w[0].1.haversine_m(w[1].1)).sum()
}

/// All pistes, piste relations and site relations of some OSM data.
#[derive(Debug, Clone, Default)]
pub struct PisteSet {
    /// Piste ways.
    pub pistes: Vec<Piste>,
    /// Piste relations.
    pub relations: Vec<PisteRelation>,
    /// `site=piste` relations.
    pub sites: Vec<SiteRelation>,
}

fn is_piste_relation(tags: &Tags) -> bool {
    !uses_of(tags).is_empty() || (tags.is("route", "ski") && tags.get("piste:type").is_none())
}

fn relation_uses(tags: &Tags) -> BTreeSet<Use> {
    let u = uses_of(tags);
    if u.is_empty() && tags.is("route", "ski") {
        BTreeSet::from([Use::Nordic])
    } else {
        u
    }
}

fn name_of(tags: &Tags) -> Option<String> {
    tags.non_empty("piste:name")
        .or_else(|| tags.non_empty("name"))
        .map(str::to_owned)
}

fn lit_of(tags: &Tags) -> Option<bool> {
    match tags.get("piste:lit").or_else(|| tags.get("lit")) {
        Some("yes") => Some(true),
        Some("no") => Some(false),
        _ => None,
    }
}

/// Classifies every piste of `data`.
pub fn classify(data: &OsmData) -> PisteSet {
    let mut relations: Vec<PisteRelation> = Vec::new();
    for r in &data.relations {
        if !is_piste_relation(&r.tags) {
            continue;
        }
        let colour_raw = r.tags.non_empty("colour").map(str::to_owned);
        let colour = colour_raw.as_deref().and_then(Colour::parse);
        let difficulty_tag = r.tags.get("piste:difficulty").and_then(Difficulty::parse);
        relations.push(PisteRelation {
            id: r.id,
            name: name_of(&r.tags),
            ref_: r
                .tags
                .non_empty("piste:ref")
                .or_else(|| r.tags.non_empty("ref"))
                .map(str::to_owned),
            colour,
            colour_raw,
            difficulty_tag,
            difficulty: colour.and_then(Colour::difficulty).or(difficulty_tag),
            uses: relation_uses(&r.tags),
            grooming: r.tags.non_empty("piste:grooming").map(str::to_owned),
            ways: r
                .members
                .iter()
                .filter(|m| m.kind == MemberKind::Way)
                .map(|m| (m.id, m.role.clone()))
                .collect(),
        });
    }
    relations.sort_by_key(|r| r.id);

    let mut rel_of_way: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, r) in relations.iter().enumerate() {
        for (w, _) in &r.ways {
            let v = rel_of_way.entry(*w).or_default();
            if !v.contains(&i) {
                v.push(i);
            }
        }
    }

    let mut pistes = Vec::new();
    for w in &data.ways {
        if w.tags.is("area", "yes") {
            continue;
        }
        let own = uses_of(&w.tags);
        let rels = rel_of_way.get(&w.id).cloned().unwrap_or_default();
        if own.is_empty() && rels.is_empty() {
            continue;
        }
        let points = data.way_points(w);
        if points.len() < 2 {
            continue;
        }
        let mut uses = own.clone();
        for &i in &rels {
            uses.extend(relations[i].uses.iter().copied());
        }
        let way_difficulty = w.tags.get("piste:difficulty").and_then(Difficulty::parse);
        let from_colour: Vec<Difficulty> = rels
            .iter()
            .filter_map(|&i| relations[i].colour.and_then(Colour::difficulty))
            .collect();
        let from_tag: Vec<Difficulty> = rels
            .iter()
            .filter_map(|&i| relations[i].difficulty_tag)
            .collect();
        let (diffs, source) = if !from_colour.is_empty() {
            (from_colour, Some(DifficultySource::RelationColour))
        } else if !from_tag.is_empty() {
            (from_tag, Some(DifficultySource::RelationDifficulty))
        } else if let Some(d) = way_difficulty {
            (vec![d], Some(DifficultySource::Way))
        } else {
            (Vec::new(), None)
        };
        let grooming = w
            .tags
            .non_empty("piste:grooming")
            .map(Grooming::parse)
            .or_else(|| {
                rels.iter()
                    .find_map(|&i| relations[i].grooming.as_deref().map(Grooming::parse))
            })
            .unwrap_or_default();
        let length_m = polyline_length_m(&points);
        pistes.push(Piste {
            way_id: w.id,
            tagged: !own.is_empty(),
            uses,
            way_name: name_of(&w.tags),
            relations: rels,
            difficulty: diffs.iter().min().copied(),
            difficulty_max: diffs.iter().max().copied(),
            difficulty_source: source,
            way_difficulty,
            grooming,
            oneway: Oneway::parse(w.tags.get("piste:oneway")),
            lit: lit_of(&w.tags),
            points,
            length_m,
        });
    }

    // Site relations: ways reached through any depth of member relations, stopping at
    // nested sites (the inner site is the more specific grouping).
    let is_site = |id: i64| {
        data.relation(id)
            .is_some_and(|r| r.tags.is("site", "piste"))
    };
    let mut sites = Vec::new();
    let mut parents: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for r in &data.relations {
        if !r.tags.is("site", "piste") {
            continue;
        }
        let mut ways = BTreeSet::new();
        let mut stack = vec![r.id];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(rel) = data.relation(id) else {
                continue;
            };
            for m in &rel.members {
                match m.kind {
                    MemberKind::Way => {
                        ways.insert(m.id);
                    }
                    MemberKind::Relation if m.id != r.id && is_site(m.id) => {
                        parents.entry(m.id).or_default().push(r.id);
                    }
                    MemberKind::Relation => stack.push(m.id),
                    MemberKind::Node => {}
                }
            }
        }
        sites.push(SiteRelation {
            id: r.id,
            name: r.tags.non_empty("name").map(str::to_owned),
            ways,
            parents: Vec::new(),
        });
    }
    // A site whose ways are a strict subset of another's is a sub-site of it (sites often
    // list the same route relations: « Le Risoux » inside « Les Rousses »).
    let mut sites_of_way: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, s) in sites.iter().enumerate() {
        for w in &s.ways {
            sites_of_way.entry(*w).or_default().push(i);
        }
    }
    let mut pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
    for v in sites_of_way.values() {
        for &a in v {
            for &b in v {
                if a != b {
                    pairs.insert((a, b));
                }
            }
        }
    }
    for (a, b) in pairs {
        let (sa, sb) = (&sites[a], &sites[b]);
        if sa.ways.len() < sb.ways.len() && sa.ways.is_subset(&sb.ways) {
            let (child, parent) = (sa.id, sb.id);
            parents.entry(child).or_default().push(parent);
        }
    }
    for s in &mut sites {
        if let Some(p) = parents.remove(&s.id) {
            s.parents = p;
            s.parents.sort_unstable();
            s.parents.dedup();
        }
    }
    sites.sort_by_key(|s| s.id);

    PisteSet {
        pistes,
        relations,
        sites,
    }
}

impl PisteSet {
    /// Pistes having at least one vertex in `bbox`, with the relations they use.
    pub fn in_bbox(&self, bbox: &BBox) -> PisteSet {
        let pistes: Vec<Piste> = self
            .pistes
            .iter()
            .filter(|p| p.touches(bbox))
            .cloned()
            .collect();
        self.subset(pistes)
    }

    /// Re-indexes relations for a subset of the pistes.
    pub fn subset(&self, mut pistes: Vec<Piste>) -> PisteSet {
        let used: BTreeSet<usize> = pistes.iter().flat_map(|p| p.relations.clone()).collect();
        let remap: BTreeMap<usize, usize> = used.iter().enumerate().map(|(j, &i)| (i, j)).collect();
        for p in &mut pistes {
            p.relations = p.relations.iter().map(|i| remap[i]).collect();
        }
        let way_ids: BTreeSet<i64> = pistes.iter().map(|p| p.way_id).collect();
        PisteSet {
            relations: used.iter().map(|&i| self.relations[i].clone()).collect(),
            sites: self
                .sites
                .iter()
                .filter(|s| s.ways.iter().any(|w| way_ids.contains(w)))
                .cloned()
                .collect(),
            pistes,
        }
    }

    /// Display name of a piste: its own name, else the names of its relations.
    pub fn display_name(&self, p: &Piste) -> Option<String> {
        if let Some(n) = &p.way_name {
            return Some(n.clone());
        }
        let names: BTreeSet<&str> = p
            .relations
            .iter()
            .filter_map(|&i| self.relations[i].name.as_deref())
            .collect();
        (!names.is_empty()).then(|| names.into_iter().collect::<Vec<_>>().join(" / "))
    }

    /// Colours of the relations of a piste (deduplicated, ordered).
    pub fn colours(&self, p: &Piste) -> Vec<Colour> {
        let c: BTreeSet<Colour> = p
            .relations
            .iter()
            .filter_map(|&i| self.relations[i].colour)
            .collect();
        c.into_iter().collect()
    }

    /// Total length in km of the pistes matching `f`.
    pub fn km(&self, f: impl Fn(&Piste) -> bool) -> f64 {
        self.pistes
            .iter()
            .filter(|p| f(p))
            .map(|p| p.length_m)
            .sum::<f64>()
            / 1000.0
            + 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Member, Relation, Way};

    fn tags(kv: &[(&str, &str)]) -> Tags {
        Tags(
            kv.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn colours() {
        assert_eq!(Colour::parse("green"), Some(Colour::Green));
        assert_eq!(Colour::parse(" Rouge "), Some(Colour::Red));
        assert_eq!(Colour::parse("#FF0000"), Some(Colour::Red));
        assert_eq!(Colour::parse("#00ff00"), Some(Colour::Green));
        assert_eq!(Colour::parse("#008000"), Some(Colour::Green));
        assert_eq!(Colour::parse("#00f"), Some(Colour::Blue));
        assert_eq!(Colour::parse("#000000"), Some(Colour::Black));
        assert_eq!(Colour::parse("#222"), Some(Colour::Black));
        assert_eq!(Colour::parse("#FFFF00"), Some(Colour::Other));
        assert_eq!(Colour::parse("#ffffff"), Some(Colour::Other));
        assert_eq!(Colour::parse("orange"), Some(Colour::Other));
        assert_eq!(Colour::parse(""), None);
        assert_eq!(Colour::Black.difficulty(), Some(Difficulty::Advanced));
        assert_eq!(Colour::Green.difficulty(), Some(Difficulty::Novice));
        assert_eq!(Colour::Other.difficulty(), None);
    }

    #[test]
    fn grooming_and_oneway() {
        let g = Grooming::parse("classic+skating");
        assert!(g.classic && g.skating && !g.backcountry);
        let g = Grooming::parse("backcountry");
        assert!(g.backcountry && !g.classic);
        assert_eq!(Oneway::parse(Some("-1")), Oneway::Backward);
        assert_eq!(Oneway::parse(Some("yes")), Oneway::Forward);
        assert_eq!(Oneway::parse(None), Oneway::Unknown);
    }

    /// Two loops share way 2: a black loop (relation 100) and a green loop (relation 101).
    /// Way 2 says `easy`; way 3 has no piste tag at all but is in the black loop.
    fn sample() -> OsmData {
        let nodes = (1..=5).map(|i| (i, LonLat::new(6.0 + i as f64 * 0.001, 46.6)));
        let ways = vec![
            Way {
                id: 1,
                nodes: vec![1, 2],
                tags: tags(&[("piste:type", "nordic"), ("piste:grooming", "classic")]),
            },
            Way {
                id: 2,
                nodes: vec![2, 3],
                tags: tags(&[
                    ("piste:type", "nordic"),
                    ("piste:difficulty", "easy"),
                    ("piste:oneway", "yes"),
                ]),
            },
            Way {
                id: 3,
                nodes: vec![3, 4],
                tags: tags(&[("highway", "secondary")]),
            },
            Way {
                id: 4,
                nodes: vec![4, 5],
                tags: tags(&[("highway", "track")]),
            },
            Way {
                id: 5,
                nodes: vec![4, 5],
                tags: tags(&[("piste:type", "hike"), ("name", "Raquettes")]),
            },
        ];
        let member = |id| Member {
            kind: MemberKind::Way,
            id,
            role: String::new(),
        };
        let relations = vec![
            Relation {
                id: 100,
                members: vec![member(2), member(3)],
                tags: tags(&[
                    ("type", "route"),
                    ("route", "piste"),
                    ("piste:type", "nordic"),
                    ("name", "Greffier"),
                    ("colour", "black"),
                    ("piste:grooming", "classic;skating"),
                ]),
            },
            Relation {
                id: 101,
                members: vec![member(1), member(2)],
                tags: tags(&[
                    ("type", "route"),
                    ("route", "ski"),
                    ("piste:name", "Les Mortes"),
                    ("colour", "#008000"),
                ]),
            },
            Relation {
                id: 200,
                members: vec![Member {
                    kind: MemberKind::Relation,
                    id: 100,
                    role: String::new(),
                }],
                tags: tags(&[("type", "site"), ("site", "piste"), ("name", "Chapelle")]),
            },
        ];
        OsmData::from_elements(nodes, ways, relations)
    }

    #[test]
    fn relation_attributes_override_ways() {
        let set = classify(&sample());
        let ids: Vec<i64> = set.pistes.iter().map(|p| p.way_id).collect();
        // Way 4 is neither tagged nor in a piste relation.
        assert_eq!(ids, vec![1, 2, 3, 5]);
        let p2 = &set.pistes[1];
        assert_eq!(p2.relations.len(), 2);
        assert_eq!(p2.difficulty, Some(Difficulty::Novice), "easiest relation");
        assert_eq!(p2.difficulty_max, Some(Difficulty::Advanced));
        assert_eq!(p2.difficulty_source, Some(DifficultySource::RelationColour));
        assert_eq!(p2.way_difficulty, Some(Difficulty::Easy));
        assert_eq!(p2.oneway, Oneway::Forward);
        assert_eq!(
            set.display_name(p2).as_deref(),
            Some("Greffier / Les Mortes")
        );
        assert_eq!(set.colours(p2), vec![Colour::Green, Colour::Black]);
        // Way 3: only through the relation; grooming inherited from it.
        let p3 = &set.pistes[2];
        assert!(!p3.tagged && p3.is_nordic());
        assert_eq!(p3.difficulty, Some(Difficulty::Advanced));
        assert!(p3.grooming.classic && p3.grooming.skating);
        // Way 1 keeps its own grooming; `route=ski` without piste:type is nordic.
        let p1 = &set.pistes[0];
        assert!(p1.grooming.classic && !p1.grooming.skating);
        assert_eq!(p1.difficulty, Some(Difficulty::Novice));
        // Hike way.
        let p5 = &set.pistes[3];
        assert_eq!(p5.uses, BTreeSet::from([Use::Hike]));
        assert_eq!(p5.difficulty, None);
        // Site relation reaches ways through its member relation.
        assert_eq!(set.sites.len(), 1);
        assert_eq!(set.sites[0].ways, BTreeSet::from([2, 3]));
        // Length: 0.001° of longitude at 46.6° N ≈ 76.5 m.
        assert!((p1.length_m - 76.5).abs() < 0.5, "{}", p1.length_m);
    }

    #[test]
    fn nested_sites_keep_their_own_ways() {
        let d = sample();
        let mut rels = d.relations.clone();
        rels.push(Relation {
            id: 201,
            members: vec![
                Member {
                    kind: MemberKind::Relation,
                    id: 200,
                    role: String::new(),
                },
                Member {
                    kind: MemberKind::Way,
                    id: 5,
                    role: String::new(),
                },
            ],
            tags: tags(&[("type", "site"), ("site", "piste"), ("name", "Massif")]),
        });
        let nodes: Vec<(i64, LonLat)> = (1..=5).map(|i| (i, d.node(i).unwrap())).collect();
        let d = OsmData::from_elements(nodes, d.ways.clone(), rels);
        let set = classify(&d);
        assert_eq!(set.sites.len(), 2);
        let (inner, outer) = (&set.sites[0], &set.sites[1]);
        assert_eq!(inner.id, 200);
        assert_eq!(inner.ways, BTreeSet::from([2, 3]));
        assert_eq!(inner.parents, vec![201]);
        assert_eq!(outer.ways, BTreeSet::from([5]));
        assert!(outer.parents.is_empty());
    }

    #[test]
    fn subset_reindexes_relations() {
        let set = classify(&sample());
        let sub = set.subset(vec![set.pistes[2].clone()]);
        assert_eq!(sub.relations.len(), 1);
        assert_eq!(sub.relations[0].id, 100);
        assert_eq!(sub.pistes[0].relations, vec![0]);
        assert_eq!(sub.sites.len(), 1);
    }
}
