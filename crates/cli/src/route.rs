//! `skimap route`: route on the trails of a site (C1), with the estimated time (C5) and,
//! given a start time, the turnaround before nightfall (C7). Reads the graph the phone
//! reads (`routes/piste_graph.json`, built by `skimap routing`), writes a GeoJSON.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde_json::{Map, Value, json};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::output::{feature, point, write_geojson};
use skimap_common::site::Site;
use skimap_core::ephemeris::local_time;
use skimap_core::eta::EtaModel;
use skimap_core::graph::PisteGraph;
use skimap_core::routing::{self, Router, Style};
use skimap_core::turnaround::{TurnaroundParams, deadline, out_and_back};

/// Parses `lat,lon` (WGS84 degrees).
pub fn parse_lat_lon(s: &str) -> Result<LonLat> {
    let bad = || anyhow!("position `{s}` (attendu lat,lon en degrés, ex. 44.8589,6.7250)");
    let (lat, lon) = s.split_once(',').ok_or_else(bad)?;
    let (lat, lon): (f64, f64) = (
        lat.trim().parse().map_err(|_| bad())?,
        lon.trim().parse().map_err(|_| bad())?,
    );
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(bad());
    }
    Ok(LonLat::new(lon, lat))
}

/// Start instant: `HH:MM` in French legal time on `date` (default: today), or anything
/// [`crate::build::parse_instant`] reads.
pub fn parse_start(s: &str, date: Option<NaiveDate>) -> Result<DateTime<Utc>> {
    if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M") {
        let d = date.unwrap_or_else(|| local_time(Utc::now()).date());
        return crate::build::parse_instant(&format!("{}T{}", d, t.format("%H:%M")));
    }
    crate::build::parse_instant(s)
        .with_context(|| format!("départ `{s}` (HH:MM, ou date et heure)"))
}

/// Request of `skimap route`.
#[derive(Debug, Clone)]
pub struct RouteRequest {
    /// Start.
    pub from: LonLat,
    /// End.
    pub to: LonLat,
    /// Technique (cost and time model).
    pub style: Style,
    /// Start time, for the turnaround (C7).
    pub start: Option<DateTime<Utc>>,
    /// Largest distance from a point to the trails, metres.
    pub max_snap_m: f64,
    /// GeoJSON written.
    pub out: PathBuf,
}

fn style_fr(s: Style) -> &'static str {
    match s {
        Style::Classic => "classique",
        Style::Skating => "skating",
    }
}

fn hm(t: DateTime<Utc>) -> String {
    local_time(t).format("%H:%M").to_string()
}

fn duration(s: f64) -> String {
    let m = (s / 60.0).round() as i64;
    format!("{}h{:02}", m / 60, m % 60)
}

/// Loads `routes/piste_graph.json` of `site`.
pub fn load_graph(layout: &DataLayout, site: &Site) -> Result<PisteGraph> {
    let p = layout
        .layer_dir(site, routing::LAYER)?
        .join(routing::GRAPH_FILE);
    let s = std::fs::read_to_string(&p).with_context(|| {
        format!(
            "{} absent : construire d'abord le graphe (`skimap routing {}`)",
            p.display(),
            site.id
        )
    })?;
    Ok(PisteGraph::from_json(&s)?)
}

/// Routes `req` on `g`: fastest route for the style (C5 time as cost), its length, climb,
/// descent and time; with a start time, the deadline of the day (C7), the arrival and the
/// latest turnaround point when the route is skied out and back. Writes the GeoJSON and
/// returns the lines to print.
pub fn route(g: &PisteGraph, site: &Site, req: &RouteRequest) -> Result<Vec<String>> {
    let model = EtaModel::new(req.style);
    let r = Router::new(g, model)
        .route(req.from, req.to, req.max_snap_m)
        .with_context(|| format!("itinéraire sur les pistes de {}", site.id))?;
    let pieces = r.pieces(g);
    let b = model.time_s(&pieces);
    let (up, down) = r.gain_loss(g);
    let names = r.names(g);
    let mut lines = vec![
        format!(
            "{} → {} ({}, style {}) : {:.2} km, D+ {up:.0} m, D− {down:.0} m",
            fmt_pos(req.from),
            fmt_pos(req.to),
            site.id,
            style_fr(req.style),
            r.length_m() / 1000.0
        ),
        format!(
            "temps estimé (C5) : {} (montée {}, plat {}, descente {}), {:.1} km/h de moyenne",
            duration(b.total_s),
            duration(b.uphill_s),
            duration(b.flat_s),
            duration(b.downhill_s),
            b.mean_kmh()
        ),
        format!(
            "rattachement aux pistes : départ à {:.0} m, arrivée à {:.0} m ; relief connu sur {:.0} % du trajet",
            r.start.dist_m,
            r.end.dist_m,
            r.profile_coverage(g) * 100.0
        ),
        format!("pistes : {}", names.join(", ")),
    ];
    let mut props = Map::new();
    props.insert("kind".into(), json!("route"));
    props.insert("site".into(), json!(site.id));
    props.insert("style".into(), json!(req.style));
    props.insert("from".into(), json!([req.from.lon, req.from.lat]));
    props.insert("to".into(), json!([req.to.lon, req.to.lat]));
    props.insert("eta_s".into(), json!(b.total_s.round()));
    props.insert("eta".into(), json!(duration(b.total_s)));
    props.insert("eta_breakdown".into(), serde_json::to_value(b)?);
    let mut extra = Vec::new();
    if let Some(start) = req.start {
        let tp = TurnaroundParams::default();
        let date = local_time(start).date();
        let dl = deadline(date, r.start.pos, &tp)
            .ok_or_else(|| anyhow!("pas de coucher de soleil le {date}"))?;
        let arrival = start + chrono::Duration::seconds((b.total_s * tp.eta_factor) as i64);
        let margin = (dl.deadline - arrival).num_seconds() as f64;
        lines.push(format!(
            "C7, départ {} le {date} : coucher du soleil {}, fin du crépuscule civil {}, retour au plus tard {} (crépuscule − {:.0} min) ; arrivée prévue {} (temps × {}), {} {}",
            hm(start),
            hm(dl.sunset),
            hm(dl.civil_dusk),
            hm(dl.deadline),
            tp.margin_min,
            hm(arrival),
            tp.eta_factor,
            if margin >= 0.0 { "marge" } else { "RETARD" },
            duration(margin.abs()),
        ));
        let mut turn = json!({
            "start_local": local_time(start).format("%Y-%m-%dT%H:%M").to_string(),
            "sunset_local": hm(dl.sunset),
            "civil_dusk_local": hm(dl.civil_dusk),
            "deadline_local": hm(dl.deadline),
            "arrival_local": hm(arrival),
            "margin_s": margin.round(),
            "params": tp,
        });
        match out_and_back(g, &r, &model, start, &dl, &tp) {
            Some(ob) => {
                lines.push(if ob.whole_route {
                    format!(
                        "aller-retour sur le même trajet : possible en entier, retour {}",
                        hm(ob.whole_arrival)
                    )
                } else {
                    format!(
                        "aller-retour sur le même trajet : demi-tour au plus tard au km {:.1} sur {:.1}, à {}, retour {} (le trajet entier ramènerait à {})",
                        ob.turn_at_m / 1000.0,
                        r.length_m() / 1000.0,
                        hm(ob.turn_time),
                        hm(ob.arrival),
                        hm(ob.whole_arrival)
                    )
                });
                turn["out_and_back"] = json!({
                    "whole_route": ob.whole_route,
                    "turn_at_m": ob.turn_at_m.round(),
                    "turn_local": hm(ob.turn_time),
                    "arrival_local": hm(ob.arrival),
                    "whole_arrival_local": hm(ob.whole_arrival),
                });
                if !ob.whole_route {
                    extra.push(feature(
                        point(ob.turn_pos),
                        json!({"kind": "turnaround", "turn_at_m": ob.turn_at_m.round(), "turn_local": hm(ob.turn_time)}),
                    ));
                }
            }
            None => {
                lines.push("aller-retour sur le même trajet : départ déjà trop tard".into());
                turn["out_and_back"] = Value::Null;
            }
        }
        props.insert("turnaround".into(), turn);
    }
    let mut features = vec![r.to_feature(g, props)];
    features.push(feature(
        point(r.start.pos),
        json!({"kind": "start", "snap_m": (r.start.dist_m * 10.0).round() / 10.0}),
    ));
    features.push(feature(
        point(r.end.pos),
        json!({"kind": "end", "snap_m": (r.end.dist_m * 10.0).round() / 10.0}),
    ));
    features.extend(extra);
    write_out(&req.out, &features)?;
    lines.push(format!("GeoJSON : {}", req.out.display()));
    Ok(lines)
}

fn fmt_pos(p: LonLat) -> String {
    format!("{:.5},{:.5}", p.lat, p.lon)
}

fn write_out(path: &Path, features: &[Value]) -> Result<()> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        DataLayout::ensure(d)?;
    }
    write_geojson(path, features)?;
    Ok(())
}

/// Parses `classic` / `skating`.
pub fn parse_style(s: &str) -> Result<Style> {
    match s {
        "classic" | "classique" => Ok(Style::Classic),
        "skating" | "skate" => Ok(Style::Skating),
        _ => bail!("style `{s}` (classic ou skating)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_and_starts() {
        let p = parse_lat_lon("44.8589,6.7250").unwrap();
        assert_eq!((p.lat, p.lon), (44.8589, 6.725));
        assert!(parse_lat_lon("6.7250").is_err());
        assert!(parse_lat_lon("144.0,6.7").is_err());
        let d = NaiveDate::from_ymd_opt(2027, 1, 15);
        // Winter: 13:00 legal time is 12:00 UTC.
        assert_eq!(
            parse_start("13:00", d).unwrap().to_rfc3339(),
            "2027-01-15T12:00:00+00:00"
        );
        assert_eq!(
            parse_start("2027-01-15T13:00", None).unwrap(),
            parse_start("13:00", d).unwrap()
        );
        assert!(parse_start("1pm", d).is_err());
        assert_eq!(parse_style("skating").unwrap(), Style::Skating);
        assert!(parse_style("alpin").is_err());
    }

    #[test]
    fn route_needs_the_graph() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path());
        let e = load_graph(
            &layout,
            skimap_common::site::site("cervieres").expect("evaluation site"),
        )
        .unwrap_err();
        assert!(
            format!("{e:#}").contains("skimap routing cervieres"),
            "{e:#}"
        );
    }
}
