//! Tracks with a published rating (calibration references) and tracks to review.
//!
//! Sources:
//!
//! - the SRN layer of G5 (`data/out/france/srn/srn_routes.geojson`): every route, with
//!   the published level when the provider gives one (Geotrek of the PNR du Vercors:
//!   « niveau 1 » to « niveau 4 » = Facile, Moyen, Difficile, Très difficile, from its
//!   `trek_difficulty` API; OSM `piste:difficulty`; Camptocamp « ski 1.x », the ski
//!   touring scale, kept apart);
//! - skirandonneenordique.com, the only SRN topo base (research report 04): « Facile »,
//!   « Moyenne », « Difficile » and the GPX of each topo; no licence on the site, so the
//!   tracks stay in the downloader cache and only the evaluation figures are published;
//! - the off-piste routes computed by C3 (`data/out/<site>/routes/offpiste_*.geojson`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;

use crate::{Error, Result};

/// skirandonneenordique.com source name in the downloader.
pub const SRN_SOURCE: &str = "skirandonneenordique";
/// Topo list of skirandonneenordique.com.
pub const SRN_TOPOS: &str = "https://www.skirandonneenordique.com/topos/";
const CACHE_DAYS: u64 = 30;

/// A published rating.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Published {
    /// Scale: `srn_com` (3 levels), `geotrek_vercors` (4 levels), `osm` (piste:difficulty),
    /// `camptocamp` (ski touring 1.1 to 5.x, not mapped).
    pub scale: String,
    /// Text as published.
    pub raw: String,
    /// Level 1–4 when the scale maps onto the SRN levels.
    pub level: Option<u8>,
}

/// A reference or review track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefRoute {
    /// Identifier (`<provider>:<id>`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Provider.
    pub provider: String,
    /// Page.
    pub url: String,
    /// Licence of the track.
    pub licence: String,
    /// Published ratings (one per source; several when sources agree on the track).
    pub ratings: Vec<Published>,
    /// Region (massif or zone as published).
    pub region: String,
    /// Published length, km.
    pub published_km: Option<f64>,
    /// Published ascent, m.
    pub published_ascent_m: Option<f64>,
    /// Track, WGS84 `[lon, lat]`.
    pub track: Vec<[f64; 2]>,
    /// Number of polylines chained into the track.
    pub parts: usize,
    /// Straight jumps added between the parts, m.
    pub jumps_m: f64,
}

impl RefRoute {
    /// Track as positions.
    pub fn positions(&self) -> Vec<LonLat> {
        self.track.iter().map(|p| LonLat::new(p[0], p[1])).collect()
    }
}

/// Chains polylines into one track: longest first, then the nearest end each time.
pub fn chain(mut parts: Vec<Vec<LonLat>>) -> (Vec<LonLat>, f64) {
    parts.retain(|p| p.len() >= 2);
    if parts.is_empty() {
        return (Vec::new(), 0.0);
    }
    let len = |p: &Vec<LonLat>| p.windows(2).map(|w| w[0].haversine_m(w[1])).sum::<f64>();
    parts.sort_by(|a, b| len(b).total_cmp(&len(a)));
    let mut out = parts.remove(0);
    let mut jumps = 0.0;
    while !parts.is_empty() {
        let (head, tail) = (out[0], *out.last().expect("non-empty"));
        let mut best = (f64::INFINITY, 0usize, false, false);
        for (i, p) in parts.iter().enumerate() {
            let (a, b) = (p[0], *p.last().expect("two points"));
            for (d, at_end, rev) in [
                (tail.haversine_m(a), true, false),
                (tail.haversine_m(b), true, true),
                (head.haversine_m(b), false, false),
                (head.haversine_m(a), false, true),
            ] {
                if d < best.0 {
                    best = (d, i, at_end, rev);
                }
            }
        }
        let (d, i, at_end, rev) = best;
        let mut p = parts.remove(i);
        if rev {
            p.reverse();
        }
        jumps += d;
        if at_end {
            out.extend(p);
        } else {
            p.extend(out);
            out = p;
        }
    }
    (out, jumps)
}

fn coords_parts(geom: &serde_json::Value) -> Vec<Vec<LonLat>> {
    let line = |v: &serde_json::Value| -> Vec<LonLat> {
        v.as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|c| Some(LonLat::new(c[0].as_f64()?, c[1].as_f64()?)))
                    .collect()
            })
            .unwrap_or_default()
    };
    match geom["type"].as_str() {
        Some("LineString") => vec![line(&geom["coordinates"])],
        Some("MultiLineString") => geom["coordinates"]
            .as_array()
            .map(|ls| ls.iter().map(line).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Published rating of a G5 route (`difficulty` property).
pub fn g5_rating(provider: &str, difficulty: &str) -> Option<Published> {
    let d = difficulty.trim();
    match provider {
        "geotrek" => {
            let n: u8 = d.strip_prefix("niveau ")?.trim().parse().ok()?;
            Some(Published {
                scale: "geotrek_vercors".into(),
                raw: d.into(),
                level: (1..=4).contains(&n).then_some(n),
            })
        }
        "osm" => {
            let level = match d {
                "novice" | "easy" => 1,
                "intermediate" => 2,
                "advanced" => 3,
                "expert" | "freeride" | "extreme" => 4,
                _ => return None,
            };
            Some(Published {
                scale: "osm".into(),
                raw: d.into(),
                level: Some(level),
            })
        }
        "camptocamp" => Some(Published {
            scale: "camptocamp".into(),
            raw: d.into(),
            level: None,
        }),
        _ => None,
    }
}

/// Routes of the G5 layer (all of them; ratings when published).
pub fn g5_routes(layout: &DataLayout) -> Result<Vec<RefRoute>> {
    let path = layout
        .france_layer_dir("srn")
        .map_err(|e| Error::Data(e.to_string()))?
        .join("srn_routes.geojson");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::Data(format!("{}: {e}", path.display())))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| Error::Data(e.to_string()))?;
    let mut out = Vec::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_owned();
        let (track, jumps) = chain(coords_parts(&f["geometry"]));
        if track.len() < 2 {
            continue;
        }
        let provider = s("provider");
        let ratings = g5_rating(&provider, &s("difficulty")).into_iter().collect();
        out.push(RefRoute {
            id: s("id"),
            name: s("name"),
            provider,
            url: s("url"),
            licence: s("licence"),
            ratings,
            region: s("activity"),
            published_km: p["published_length_km"].as_f64(),
            published_ascent_m: p["ascent_m"].as_f64(),
            parts: coords_parts(&f["geometry"]).len(),
            jumps_m: jumps,
            track: track.iter().map(|q| [q.lon, q.lat]).collect(),
        });
    }
    Ok(out)
}

/// Visible lines of an HTML page (tags removed, entities of the topo pages decoded).
pub fn html_lines(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut skip_until: Option<&[u8]> = None;
    let lower = html.to_ascii_lowercase().into_bytes();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if let Some(end) = skip_until {
            if lower[i..].starts_with(end) {
                skip_until = None;
                i += end.len();
            } else {
                i += 1;
            }
            continue;
        }
        if !in_tag && lower[i..].starts_with(b"<script") {
            skip_until = Some(b"</script>");
            continue;
        }
        if !in_tag && lower[i..].starts_with(b"<style") {
            skip_until = Some(b"</style>");
            continue;
        }
        let c = bytes[i];
        if c == b'<' {
            in_tag = true;
            text.push('\n');
        } else if c == b'>' && in_tag {
            in_tag = false;
        } else if !in_tag {
            let ch_len = html[i..].chars().next().map_or(1, char::len_utf8);
            text.push_str(&html[i..i + ch_len]);
            i += ch_len;
            continue;
        }
        i += 1;
    }
    text.lines()
        .map(|l| {
            l.replace("&nbsp;", " ")
                .replace("&#39;", "'")
                .replace("&#x27;", "'")
                .replace("&gt;", ">")
                .replace("&lt;", "<")
                .replace("&amp;", "&")
                .replace("&quot;", "\"")
                .replace("&rsquo;", "’")
                .replace('\u{a0}', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect()
}

fn field_after(lines: &[String], key: &str) -> Option<String> {
    lines
        .iter()
        .position(|l| l.trim_end_matches(':').trim() == key)
        .and_then(|i| lines.get(i + 1).cloned())
}

fn number(s: &str) -> Option<f64> {
    let t: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .collect();
    t.replace(',', ".").parse().ok()
}

/// A skirandonneenordique.com topo page.
#[derive(Debug, Clone, PartialEq)]
pub struct Topo {
    /// Slug.
    pub slug: String,
    /// Title.
    pub title: String,
    /// « France > Vercors ».
    pub zone: String,
    /// « Facile », « Moyenne », « Difficile ».
    pub difficulty: String,
    /// km.
    pub distance_km: Option<f64>,
    /// m.
    pub ascent_m: Option<f64>,
    /// GPX path on the site.
    pub gpx: Option<String>,
}

/// Parses a topo page.
pub fn parse_topo(slug: &str, html: &str) -> Topo {
    let lines = html_lines(html);
    let title = lines.first().cloned().unwrap_or_default();
    let zone = lines
        .iter()
        .find(|l| l.contains(" > ") && l.len() < 80)
        .cloned()
        .unwrap_or_default();
    let gpx = html
        .split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .find(|h| h.to_ascii_lowercase().contains(".gpx"))
        .map(str::to_owned);
    Topo {
        slug: slug.to_owned(),
        title,
        zone,
        difficulty: field_after(&lines, "Difficulté").unwrap_or_default(),
        distance_km: field_after(&lines, "Distance").and_then(|s| number(&s)),
        ascent_m: field_after(&lines, "Dénivelée").and_then(|s| number(&s)),
        gpx,
    }
}

fn get_text(dl: &Downloader, url: &str) -> Result<String> {
    dl.fetch(
        &Request::get(SRN_SOURCE, url).max_age(std::time::Duration::from_secs(CACHE_DAYS * 86_400)),
    )
    .and_then(|f| f.text())
    .map_err(|e| Error::Data(format!("{url}: {e}")))
}

/// Topos of skirandonneenordique.com in France with a GPX and a rating.
pub fn srn_com_routes(dl: &Downloader) -> Result<Vec<RefRoute>> {
    let mut slugs: Vec<String> = Vec::new();
    for page in 1..=30 {
        let url = format!("{SRN_TOPOS}?page={page}");
        let html = match get_text(dl, &url) {
            Ok(h) => h,
            // The page after the last one answers 404.
            Err(e) if page > 1 && e.to_string().contains("404") => break,
            Err(e) => return Err(e),
        };
        let before = slugs.len();
        for s in html.split("href=\"/topos/").skip(1) {
            if let Some(slug) = s.split('/').next()
                && !slug.is_empty()
                && slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_alphanumeric() || c == '-')
                && !slugs.iter().any(|x| x == slug)
            {
                slugs.push(slug.to_owned());
            }
        }
        if slugs.len() == before {
            break;
        }
    }
    let mut out = Vec::new();
    for slug in slugs {
        let url = format!("{SRN_TOPOS}{slug}/");
        let topo = parse_topo(&slug, &get_text(dl, &url)?);
        let level = match topo.difficulty.as_str() {
            "Facile" => Some(1),
            "Moyenne" => Some(2),
            "Difficile" => Some(3),
            _ => None,
        };
        let (Some(gpx), Some(_)) = (&topo.gpx, level) else {
            continue;
        };
        if !topo.zone.starts_with("France") {
            continue;
        }
        let gpx_url = if gpx.starts_with("http") {
            gpx.clone()
        } else {
            format!("https://www.skirandonneenordique.com{gpx}")
        };
        let text = get_text(dl, &gpx_url)?;
        let lines = skimap_srn::datatourisme::parse_gpx(&text)
            .map_err(|e| Error::Data(format!("{gpx_url}: {e}")))?;
        let n = lines.len();
        let (track, jumps) = chain(lines);
        if track.len() < 2 {
            continue;
        }
        out.push(RefRoute {
            id: format!("srn.com:{slug}"),
            name: topo.title.clone(),
            provider: "skirandonneenordique.com".into(),
            url,
            licence: "non indiquée (usage d'évaluation, non redistribué)".into(),
            ratings: vec![Published {
                scale: "srn_com".into(),
                raw: topo.difficulty.clone(),
                level,
            }],
            region: topo.zone.clone(),
            published_km: topo.distance_km,
            published_ascent_m: topo.ascent_m,
            parts: n,
            jumps_m: jumps,
            track: track.iter().map(|q| [q.lon, q.lat]).collect(),
        });
    }
    Ok(out)
}

/// Off-piste routes computed by C3 on both sites.
pub fn c3_routes(layout: &DataLayout) -> Result<Vec<RefRoute>> {
    let mut out = Vec::new();
    for site in SITES {
        let dir = layout
            .layer_dir(site, "routes")
            .map_err(|e| Error::Data(e.to_string()))?;
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<_> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("offpiste_") && n.ends_with(".geojson"))
            .collect();
        names.sort();
        for n in names {
            let text = std::fs::read_to_string(dir.join(&n))
                .map_err(|e| Error::Data(format!("{n}: {e}")))?;
            let v: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| Error::Data(e.to_string()))?;
            let Some(f) = v["features"]
                .as_array()
                .and_then(|fs| fs.iter().find(|f| f["properties"]["kind"] == "route"))
            else {
                continue;
            };
            let (track, jumps) = chain(coords_parts(&f["geometry"]));
            let stem = n
                .trim_end_matches(".geojson")
                .trim_start_matches("offpiste_");
            out.push(RefRoute {
                id: format!("c3:{}:{stem}", site.id),
                name: format!("Hors piste calculé (C3) {} : {stem}", site.name),
                provider: "c3".into(),
                url: format!("data/out/{}/routes/{n}", site.id),
                licence: "calcul skimap (OSM ODbL, IGN Licence Ouverte)".into(),
                ratings: Vec::new(),
                region: site.name.into(),
                published_km: None,
                published_ascent_m: None,
                parts: 1,
                jumps_m: jumps,
                track: track.iter().map(|q| [q.lon, q.lat]).collect(),
            });
        }
    }
    Ok(out)
}

/// Groups of references describing the same route: two tracks are the same route when
/// at least half of the points of one lie within 150 m of the other (union-find).
pub fn same_route_groups(routes: &[RefRoute]) -> Vec<usize> {
    use skimap_common::crs;
    const R: f64 = 150.0;
    let pts: Vec<Vec<(f64, f64)>> = routes
        .iter()
        .map(|r| {
            let mut v = Vec::new();
            let mut last: Option<LonLat> = None;
            for p in r.positions() {
                if last.is_none_or(|l| l.haversine_m(p) >= 50.0) {
                    if let Ok(xy) = crs::to_lambert93(p) {
                        v.push(xy);
                    }
                    last = Some(p);
                }
            }
            v
        })
        .collect();
    type Bins = BTreeMap<(i64, i64), Vec<(f64, f64)>>;
    let bins: Vec<Bins> = pts
        .iter()
        .map(|v| {
            let mut m: Bins = BTreeMap::new();
            for &(x, y) in v {
                m.entry(((x / R).floor() as i64, (y / R).floor() as i64))
                    .or_default()
                    .push((x, y));
            }
            m
        })
        .collect();
    let near = |i: usize, j: usize| -> f64 {
        let hit = pts[i]
            .iter()
            .filter(|(x, y)| {
                let (kx, ky) = ((x / R).floor() as i64, (y / R).floor() as i64);
                (-1..=1).any(|dx| {
                    (-1..=1).any(|dy| {
                        bins[j]
                            .get(&(kx + dx, ky + dy))
                            .is_some_and(|v| v.iter().any(|(a, b)| (a - x).hypot(b - y) <= R))
                    })
                })
            })
            .count();
        hit as f64 / pts[i].len().max(1) as f64
    };
    let mut parent: Vec<usize> = (0..routes.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut k = i;
        while p[k] != r {
            let n = p[k];
            p[k] = r;
            k = n;
        }
        r
    }
    let bbox: Vec<(f64, f64, f64, f64)> = pts
        .iter()
        .map(|v| {
            v.iter().fold(
                (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(a, b, c, d), (x, y)| (a.min(*x), b.min(*y), c.max(*x), d.max(*y)),
            )
        })
        .collect();
    for i in 0..routes.len() {
        for j in i + 1..routes.len() {
            let (a, b) = (bbox[i], bbox[j]);
            if a.0 > b.2 + R || b.0 > a.2 + R || a.1 > b.3 + R || b.1 > a.3 + R {
                continue;
            }
            if near(i, j) >= 0.5 || near(j, i) >= 0.5 {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    (0..routes.len()).map(|i| find(&mut parent, i)).collect()
}

/// Share of the points of `a` (every 50 m) within 150 m of a point of `b` (brute force,
/// for a few pairs).
pub fn overlap(a: &RefRoute, b: &RefRoute) -> f64 {
    use skimap_common::crs;
    let pts = |r: &RefRoute| -> Vec<(f64, f64)> {
        let mut v = Vec::new();
        let mut last: Option<LonLat> = None;
        for p in r.positions() {
            if last.is_none_or(|l| l.haversine_m(p) >= 50.0) {
                if let Ok(xy) = crs::to_lambert93(p) {
                    v.push(xy);
                }
                last = Some(p);
            }
        }
        v
    };
    let (pa, pb) = (pts(a), pts(b));
    let hit = pa
        .iter()
        .filter(|(x, y)| pb.iter().any(|(u, v)| (u - x).hypot(v - y) <= 150.0))
        .count();
    hit as f64 / pa.len().max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_orders_and_flips_parts() {
        let a = vec![LonLat::new(0.0, 0.0), LonLat::new(0.01, 0.0)];
        let b = vec![LonLat::new(0.03, 0.0), LonLat::new(0.011, 0.0)];
        let (t, j) = chain(vec![b, a]);
        assert_eq!(t.first().unwrap().lon, 0.03);
        assert_eq!(t.last().unwrap().lon, 0.0);
        assert!(j < 120.0);
    }

    #[test]
    fn topo_fields() {
        let html = "<html><head><title>T</title><script>var a='<b>';</script></head><body><h1>Cervières - Capanna Mautino</h1>\
            <p>France &gt; Briançonnais</p><dt>Distance :</dt><dd>19,6 km</dd><dt>Difficulté :</dt><dd>Moyenne</dd>\
            <dt>Dénivelée :</dt><dd>+728 m</dd><a href=\"/media/x/trace.gpx\">GPX</a></body></html>";
        let t = parse_topo("x", html);
        assert_eq!(t.difficulty, "Moyenne");
        assert_eq!(t.zone, "France > Briançonnais");
        assert_eq!(t.distance_km, Some(19.6));
        assert_eq!(t.ascent_m, Some(728.0));
        assert_eq!(t.gpx.as_deref(), Some("/media/x/trace.gpx"));
    }

    #[test]
    fn g5_levels() {
        assert_eq!(g5_rating("geotrek", "niveau 4").unwrap().level, Some(4));
        assert_eq!(g5_rating("osm", "novice").unwrap().level, Some(1));
        assert_eq!(g5_rating("camptocamp", "ski 1.2").unwrap().level, None);
        assert!(g5_rating("osm", "").is_none());
    }
}
