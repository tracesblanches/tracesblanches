//! Builders of the pipeline layers, wired to the public functions of their crates, and
//! `build-all`, which runs them in dependency order with the time of each step.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::meta::LayerMeta;
use skimap_common::site::Site;
use skimap_core::zone_eval::{Evaluation, evaluate, legal_offset_hours};
use skimap_osm::census::{CensusOptions, CensusOutput};
use skimap_osm::extract::{PistesOutput, Planet};

/// Configuration, data layout and downloader shared by the builders.
pub struct Ctx {
    /// Process configuration.
    pub cfg: Config,
    /// Data layout.
    pub layout: DataLayout,
    /// Shared downloader.
    pub dl: Downloader,
}

impl Ctx {
    /// Context of `cfg`.
    pub fn new(cfg: &Config) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            layout: cfg.layout(),
            dl: Downloader::from_config(cfg)?,
        })
    }

    /// P2: pistes layer (reads the planet extract unless given).
    pub fn pistes(&self, site: &Site, planet: Option<&Planet>) -> Result<PistesOutput> {
        Ok(match planet {
            Some(p) => skimap_osm::extract::build_from(site, &self.layout, p)?,
            None => skimap_osm::extract::build(site, &self.layout, &self.dl)?,
        })
    }

    /// P3: census of France, `data/out/france/domaines/`.
    pub fn census(&self, planet: &Planet) -> Result<CensusOutput> {
        let opts = CensusOptions {
            personal_sources: self.cfg.personal_sources,
            ..Default::default()
        };
        Ok(skimap_osm::census::build_france_from(
            &self.layout,
            &self.dl,
            planet,
            &opts,
        )?)
    }

    /// P3: domain layer of a site from the census.
    pub fn domaines_site(
        &self,
        site: &Site,
        planet: &Planet,
        census: &CensusOutput,
    ) -> Result<PathBuf> {
        Ok(skimap_osm::census::build_site_from(
            site,
            &self.layout,
            planet,
            census,
        )?)
    }

    /// P5: terrain layer.
    pub fn terrain(&self, site: &Site) -> Result<skimap_terrain::TerrainBuild> {
        Ok(skimap_terrain::build(site, &self.layout, &self.dl)?)
    }

    /// P6: snow layer.
    pub fn snow(&self, site: &Site, date: Option<NaiveDate>) -> Result<skimap_snow::SnowBuild> {
        Ok(skimap_snow::build(site, &self.layout, &self.dl, date)?)
    }

    /// P11: huts and shelters.
    pub fn refuges(&self, site: &Site) -> Result<skimap_refuges::RefugesOutput> {
        Ok(skimap_refuges::build(site, &self.layout, &self.dl)?)
    }

    /// P9, P10: zones and rules.
    pub fn zones(&self, site: &Site) -> Result<skimap_zones::ZonesReport> {
        Ok(skimap_zones::build(site, &self.layout, &self.dl)?)
    }

    /// P12: areas without any network.
    pub fn coverage(
        &self,
        site: &Site,
        opts: &skimap_coverage::CoverageOptions,
    ) -> Result<skimap_coverage::CoverageReport> {
        Ok(skimap_coverage::build(site, &self.layout, &self.dl, opts)?)
    }

    /// P8: BERA of the massif (`date` defaults to today, UTC).
    pub fn bera(
        &self,
        site: &Site,
        date: Option<NaiveDate>,
    ) -> Result<skimap_meteo::bera::BeraFile> {
        let date = date.unwrap_or_else(|| Utc::now().date_naive());
        Ok(skimap_meteo::bera::build(
            site,
            &self.layout,
            &self.dl,
            self.cfg.meteofrance_api_key.as_ref(),
            date,
        )?)
    }

    /// P19: hourly weather (personal use only).
    pub fn meteo(&self, site: &Site) -> Result<skimap_meteo::hourly::HourlyFile> {
        Ok(skimap_meteo::hourly::build(
            site,
            &self.layout,
            &self.dl,
            self.cfg.personal_sources,
        )?)
    }

    /// P13: one poll of the trail status (personal use only).
    pub fn status_poll(&self, site: &Site) -> Result<skimap_status::PollReport> {
        Ok(skimap_status::poll(
            site,
            &self.layout,
            &self.dl,
            self.cfg.personal_sources,
        )?)
    }

    /// P20: historic avalanches, from the `pistes` layer or a saved Overpass extract.
    pub fn avalanche_historic(
        &self,
        site: &Site,
        dump: Option<&Path>,
    ) -> Result<skimap_avalanche::historic::Summary> {
        Ok(match dump {
            Some(d) => {
                skimap_avalanche::historic::build_from_dump(site, &self.layout, &self.dl, d)?
            }
            None => skimap_avalanche::historic::build(site, &self.layout, &self.dl)?,
        })
    }

    /// C1: trail graph with elevation profiles, `routes/piste_graph.json` (needs the
    /// `pistes` and `terrain` layers).
    pub fn routing(&self, site: &Site) -> Result<skimap_core::routing::GraphBuild> {
        Ok(skimap_core::routing::build(site, &self.layout)?.1)
    }

    /// P16 to P18: avalanche exposure (needs `terrain` and `pistes`; downloads the IGN
    /// LiDAR HD canopy height once).
    pub fn avalanche_exposure(
        &self,
        site: &Site,
    ) -> Result<skimap_avalanche::exposure::ExposureSummary> {
        Ok(skimap_avalanche::exposure::build(
            site,
            &self.layout,
            &self.dl,
        )?)
    }

    /// C3: off-piste rasters, `offpiste/` (needs `terrain`, `zones`, `pistes` and the snow
    /// layer directories, default the site's `snow`), with the avalanche exposure of the
    /// `avalanche-exposure` layer when it exists (router overlay by default).
    pub fn offpiste_prepare(&self, site: &Site, snow_dirs: &[PathBuf]) -> Result<LayerMeta> {
        let default;
        let dirs = if snow_dirs.is_empty() {
            default = [self.layout.layer_dir(site, "snow")?];
            &default[..]
        } else {
            snow_dirs
        };
        let exposure = match skimap_avalanche::exposure::read_exposure_grid(&self.layout, site) {
            Ok(g) => Some(g),
            Err(skimap_avalanche::Error::MissingLayer { .. }) => None,
            Err(e) => return Err(e.into()),
        };
        Ok(skimap_core::offpiste::prepare::prepare_with_exposure(
            site,
            &self.layout,
            dirs,
            exposure.as_ref(),
        )?)
    }

    /// VO3: voice clips of the pack with the names of the built layers (`pistes`,
    /// `refuges`, `zones`); Pocket TTS on this workstation (`VoiceConfig::from_env`).
    pub fn voice(
        &self,
        site: &Site,
    ) -> Result<(skimap_voice::pack::VoiceBuild, skimap_voice::PackNames)> {
        let (names, sources) = crate::voice::pack_names(&self.layout, site)?;
        let cfg = skimap_voice::VoiceConfig::from_env()?;
        let b = skimap_voice::build(site, &self.layout, &names, &sources, &cfg)?;
        Ok((b, names))
    }

    /// P14: offline pack of the site from the pipeline outputs, `data/out/<site>/pack/`
    /// (options from the configuration: personal sources).
    pub fn build_pack(&self, site: &Site) -> Result<skimap_pack::build::BuildReport> {
        let opts = skimap_pack::build::BuildOptions {
            personal_sources: self.cfg.personal_sources,
            ..Default::default()
        };
        Ok(skimap_pack::build::build(
            site,
            &self.layout,
            &self.dl,
            &opts,
        )?)
    }

    /// P21: export of the pack for Locus Map and OsmAnd, `data/out/<site>/export/`.
    pub fn export(&self, site: &Site) -> Result<skimap_pack::export::ExportReport> {
        let pack = skimap_pack::Pack::open(skimap_pack::build::pack_dir(site, &self.layout)?)?;
        Ok(skimap_pack::export::export(
            &pack,
            &self
                .layout
                .layer_dir(site, skimap_pack::export::EXPORT_LAYER)?,
        )?)
    }

    /// Runs every available builder of `site` in dependency order and prints the time and
    /// the key figures of each step. A step whose dependency failed is skipped; the
    /// personal-use steps are skipped when `personal_sources` is off. Fails if any step
    /// failed.
    pub fn build_all(&self, site: &Site) -> Result<()> {
        let mut run = Run::new(site);
        let mut planet = None;
        run.step("planet", &[], || {
            let p = skimap_osm::extract::load_planet(&self.dl)?;
            let line = format!(
                "{} pistes dans la zone France, données OSM du {}",
                p.set.pistes.len(),
                p.valid_at.as_deref().unwrap_or("?")
            );
            planet = Some(p);
            Ok(line)
        });
        let planet_ref = || planet.as_ref().ok_or_else(|| anyhow!("extrait non lu"));
        run.step("pistes", &["planet"], || {
            let s = self.pistes(site, Some(planet_ref()?))?.stats;
            Ok(format!(
                "{} ways, {} km dont {} km nordiques ; graphe {} nœuds, {} arêtes ({} connecteurs), {} composantes",
                s.ways,
                s.km_total,
                s.km_nordic,
                s.graph_nodes,
                s.graph_edges,
                s.graph_connectors,
                s.graph_components
            ))
        });
        run.step("domaines", &["planet"], || {
            let p = planet_ref()?;
            let census = self.census(p)?;
            let dir = self.domaines_site(site, p, &census)?;
            let st = &census.stats;
            let meta = LayerMeta::read(&dir.join(LayerMeta::file_name(skimap_osm::census::LAYER)))?;
            let names: Vec<String> = meta.stats["list"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| {
                    format!(
                        "{} {} km",
                        d["name"].as_str().unwrap_or("(sans nom)"),
                        d["km_nordic"]
                    )
                })
                .collect();
            Ok(format!(
                "France : {} domaines, {} km nordiques ; OpenSkiMap {}/{} ; SkiRadar {}/{} ; Nordic France {} ; site : {}",
                st["domains_france"],
                st["km_nordic_france"],
                st["openskimap"]["areas_matched_to_one_domain"],
                st["openskimap"]["areas_france_nordic"],
                st["skiradar"]["matched"],
                st["skiradar"]["stations_with_nordic"],
                match &st["nordic_france"] {
                    serde_json::Value::Null => "non (personal_sources désactivé)".to_owned(),
                    v => format!("{}/{}", v["matched"], v["stations"]),
                },
                names.join(", ")
            ))
        });
        run.step("terrain", &[], || {
            let t = self.terrain(site)?;
            let r07 = t.report07.as_ref().map_or_else(String::new, |r| {
                format!(
                    " (rapport 07 : {:.1} %, même emprise : {:.1} %)",
                    r.report * 100.0,
                    r.ours * 100.0
                )
            });
            Ok(format!(
                "{} dalles, {} x {} cellules, {:.1} % valides, pente ≥ 30° : {:.1} %{r07}",
                t.tiles,
                t.size_cells[0],
                t.size_cells[1],
                t.valid_fraction * 100.0,
                t.steep30_fraction * 100.0
            ))
        });
        run.step("snow", &["terrain"], || {
            let s = self.snow(site, None)?;
            let [mean, min, max] = s.depth_mean_min_max_cm;
            Ok(format!(
                "simulation du {}, {} cellules estimées, hauteur moyenne {mean:.1} cm (min {min:.0}, max {max:.0})",
                s.date, s.cells_estimated
            ))
        });
        run.step("refuges", &[], || {
            let st = self.refuges(site)?.stats;
            Ok(format!(
                "refuges.info {} points, OSM {} éléments, Camptocamp {} dans l'emprise, {} lieux reliés",
                st["refuges_info"]["points"],
                st["osm"]["elements"],
                st["camptocamp"]["in_box"],
                st["links"]["places"]
            ))
        });
        run.step("zones", &[], || {
            let z = self.zones(site)?;
            let kinds: Vec<String> = z.by_kind.iter().map(|(k, n)| format!("{k} {n}")).collect();
            Ok(format!("{} zones ({})", z.zones, kinds.join(", ")))
        });
        run.step("avalanche-exposure", &["terrain", "pistes"], || {
            let e = self.avalanche_exposure(site)?;
            let [low, exp, high] = e.class_shares;
            Ok(format!(
                "{} x {} cellules de {} m ; terrain peu exposé (modèle) {:.1} %, exposé {:.1} %, très exposé {:.1} % ; CLPA : {} cellules relevées",
                e.grid.0,
                e.grid.1,
                e.grid.2,
                low * 100.0,
                exp * 100.0,
                high * 100.0,
                e.clpa_cells_raised
            ))
        });
        run.step("routing", &["pistes", "terrain"], || {
            let g = self.routing(site)?;
            Ok(format!(
                "{} nœuds, {} arêtes, {:.1} km ({:.1} km à sens unique), {} composantes ; profils sur {} / {} arêtes ; {} octets",
                g.nodes,
                g.edges,
                g.length_km,
                g.oneway_km,
                g.components,
                g.profiles.edges_with_profile,
                g.edges,
                g.graph_bytes
            ))
        });
        run.step(
            "offpiste-prepare",
            &["terrain", "snow", "zones", "pistes"],
            || {
                let m = self.offpiste_prepare(site, &[])?;
                let g = &m.stats["grid"];
                let exposure = match &m.stats["exposure"] {
                    serde_json::Value::Null => "sans exposition aux avalanches".to_owned(),
                    e => format!(
                        "exposition : {} cellules très exposées, {} exposées",
                        e["cells_high"], e["cells_exposed"]
                    ),
                };
                let r = &m.stats["snow_retention"];
                let len = |v: &serde_json::Value| v.as_array().map_or(0, Vec::len);
                let snow = format!(
                    "neige : {} dates (dont {} reprises, {} supprimées)",
                    len(&m.stats["snow"]),
                    len(&r["carried_over"]),
                    len(&r["deleted"])
                );
                Ok(format!(
                    "grille {} x {} cellules de {} m, pente ≥ 30° {:.1} %, {} classes de zones (écarts de contrôle {}), {} cellules de piste damée, {exposure}, {snow}, {} fichiers",
                    g["size_cells"][0],
                    g["size_cells"][1],
                    g["cell_size"],
                    g["share_slope_ge_30_of_valid"].as_f64().unwrap_or(f64::NAN) * 100.0,
                    m.stats["zones"]["classes"],
                    m.stats["zones"]["check_mismatches"],
                    m.stats["pistes"]["cells_groomed"],
                    m.files.len()
                ))
            },
        );
        run.step("coverage", &[], || {
            let c = self.coverage(site, &skimap_coverage::CoverageOptions::default())?;
            Ok(format!(
                "ARCEP {} : partie française {} km², sans aucun réseau {} km² ({} %), {} polygones",
                c.release,
                c.analysed_km2,
                c.no_network_km2,
                c.no_network_pct,
                c.no_network_polygons
            ))
        });
        run.step("bera", &[], || {
            let b = self.bera(site, None)?;
            let risk = b
                .bulletin
                .as_ref()
                .and_then(|x| x.risk.max)
                .map_or_else(|| "-".to_owned(), |r| r.to_string());
            Ok(format!(
                "{:?}, massif {}, origine {}, risque max {risk}",
                b.status,
                b.massif.as_deref().unwrap_or("-"),
                b.origin.as_deref().unwrap_or("-")
            ))
        });
        if self.cfg.personal_sources {
            run.step("meteo", &[], || {
                let h = self.meteo(site)?;
                let hours: usize = h.points.iter().map(|p| p.hours.len()).sum();
                let first = h.points.first();
                Ok(format!(
                    "{} points, {hours} heures, du {} au {}",
                    h.points.len(),
                    first
                        .and_then(|p| p.hours.first())
                        .map_or("?", |x| x.time.as_str()),
                    first
                        .and_then(|p| p.hours.last())
                        .map_or("?", |x| x.time.as_str())
                ))
            });
            run.step("status-poll", &["pistes"], || {
                let r = self.status_poll(site)?;
                let s: Vec<String> = r
                    .sources
                    .iter()
                    .map(|s| format!("{} ({}) : {}", s.source, s.origin, s.summary))
                    .collect();
                Ok(s.join(" | "))
            });
        } else {
            run.skip(
                "meteo",
                "personal_sources désactivé (SKIMAP_PERSONAL_SOURCES)",
            );
            run.skip(
                "status-poll",
                "personal_sources désactivé (SKIMAP_PERSONAL_SOURCES)",
            );
        }
        run.step("avalanche-historic", &["pistes"], || {
            let s = self.avalanche_historic(site, None)?;
            let nets: Vec<String> = s
                .networks
                .iter()
                .map(|n| {
                    format!(
                        "{:?} {} lignes, {} km, {} km en CLPA ({} tronçons)",
                        n.network, n.lines, n.length_km, n.inside_clpa_km, n.crossings
                    )
                })
                .collect();
            Ok(format!(
                "CLPA commune {} ({} km²) ; {}",
                s.clpa_in_commune,
                s.clpa_in_commune_km2,
                nets.join(" ; ")
            ))
        });
        // The assistant reads every layer above (a missing one is said in its outputs).
        run.step("assist", &[], || {
            let (out, lines) = crate::assist::assist(
                &self.layout,
                site,
                &crate::assist::AssistArgs {
                    days: 3,
                    suggest: true,
                    outings: true,
                    ..Default::default()
                },
            )?;
            let loops: usize = out.suggestions.iter().map(|s| s.suggestions.len()).sum();
            let skipped = lines.iter().filter(|l| l.starts_with("sortie ignorée")).count();
            Ok(format!(
                "{} jours, {} conseils de fartage, {} demandes et {loops} boucles, {} listes d'équipement, {} rejeux d'alertes, {} fichiers ; {skipped} sorties sans itinéraire de preuve",
                out.briefing.days.len(),
                out.wax.len(),
                out.suggestions.len(),
                out.equipment.len(),
                out.replays.len(),
                out.files.len()
            ))
        });
        // Day plans of the preset requests (J1, J2) on the archived day of the site: the
        // trail graph, the refuges, the voice clips and `assist/history/`. Not in the pack
        // (computed in the morning, on the phone eventually: E2).
        run.step("plan", &["routing"], || {
            let r = crate::plan::plan(&self.layout, site, &crate::plan::PlanArgs::default())?;
            let missing: usize = r.plans.iter().map(|p| p.clips.missing.len()).sum();
            Ok(format!(
                "{} plans ({}), {} plans B, {} annonces prévues, {} clips absents du pack, {} fichiers",
                r.plans.len(),
                r.plans.first().map(|p| p.date.to_string()).unwrap_or_default(),
                r.plans.iter().map(|p| p.plans_b.len()).sum::<usize>(),
                r.plans.iter().map(|p| p.announcements.len()).sum::<usize>(),
                missing,
                r.files.len()
            ))
        });
        // The pack takes whatever layers exist; it does not need every step above.
        run.step("build-pack", &[], || {
            let r = self.build_pack(site)?;
            let problems = skimap_pack::Pack::open(&r.pack_dir)?.verify()?;
            Ok(format!(
                "pack {} : {:.2} Mo, {} couches, {} absentes ; relecture : {}",
                r.pack_version,
                r.total_bytes as f64 / 1e6,
                r.layers.len(),
                r.missing.len(),
                if problems.is_empty() {
                    "OK".to_owned()
                } else {
                    problems.join(" ; ")
                }
            ))
        });
        run.step("export", &["build-pack"], || {
            let r = self.export(site)?;
            Ok(format!(
                "{} fichiers, {:.2} Mo, {} couches non exportées",
                r.files.len(),
                r.total_bytes as f64 / 1e6,
                r.skipped.len()
            ))
        });
        run.finish()
    }
}

/// Steps of a `build-all` run.
struct Run<'a> {
    site: &'a Site,
    t0: Instant,
    failed: BTreeSet<&'static str>,
    lines: Vec<String>,
}

impl<'a> Run<'a> {
    fn new(site: &'a Site) -> Self {
        println!("build-all {} ({})", site.id, site.name);
        Self {
            site,
            t0: Instant::now(),
            failed: BTreeSet::new(),
            lines: Vec::new(),
        }
    }

    fn print(&mut self, line: String) {
        println!("{line}");
        self.lines.push(line);
    }

    fn step(
        &mut self,
        name: &'static str,
        deps: &[&'static str],
        f: impl FnOnce() -> Result<String>,
    ) {
        if let Some(d) = deps.iter().find(|d| self.failed.contains(*d)) {
            self.failed.insert(name);
            self.print(format!("{name:<19} {:>8}  IGNORÉ : {d} a échoué", "-"));
            return;
        }
        let t = Instant::now();
        let r = f();
        let s = t.elapsed().as_secs_f64();
        match r {
            Ok(line) => self.print(format!("{name:<19} {s:>7.2}s  {line}")),
            Err(e) => {
                self.failed.insert(name);
                self.print(format!("{name:<19} {s:>7.2}s  ÉCHEC : {e:#}"));
            }
        }
    }

    fn skip(&mut self, name: &'static str, why: &str) {
        self.print(format!("{name:<19} {:>8}  ignoré : {why}", "-"));
    }

    fn finish(self) -> Result<()> {
        let total = self.t0.elapsed().as_secs_f64();
        println!("{:<19} {total:>7.2}s", "total");
        if self.failed.is_empty() {
            Ok(())
        } else {
            bail!(
                "build-all {} : étapes en échec ou ignorées : {}",
                self.site.id,
                self.failed.iter().copied().collect::<Vec<_>>().join(", ")
            )
        }
    }
}

/// Parses `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("date `{s}` (YYYY-MM-DD)"))
}

/// Parses an ARCEP release `YYYY_TQ` (e.g. `2026_T2`).
pub fn parse_release(s: &str) -> Result<skimap_coverage::Release> {
    let bad = || anyhow!("publication ARCEP `{s}` (attendu YYYY_TQ, ex. 2026_T2)");
    let (y, q) = s.split_once("_T").ok_or_else(bad)?;
    let release = skimap_coverage::Release {
        year: y.parse().map_err(|_| bad())?,
        quarter: q.parse().map_err(|_| bad())?,
    };
    if !(1..=4).contains(&release.quarter) {
        return Err(bad());
    }
    Ok(release)
}

/// Instant of a French / Swiss legal local time (CET / CEST).
fn from_legal_time(local: NaiveDateTime) -> DateTime<Utc> {
    let guess = Utc.from_utc_datetime(&(local - ChronoDuration::hours(1)));
    let utc = Utc.from_utc_datetime(&(local - ChronoDuration::hours(legal_offset_hours(guess))));
    // Around a change of offset the first guess may be on the other side.
    Utc.from_utc_datetime(&(local - ChronoDuration::hours(legal_offset_hours(utc))))
}

/// Parses an instant: RFC 3339, or `YYYY-MM-DDTHH:MM` / `YYYY-MM-DD HH:MM` in French legal
/// time, or `YYYY-MM-DD` (noon, legal time).
pub fn parse_instant(s: &str) -> Result<DateTime<Utc>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.to_utc());
    }
    for f in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(t) = NaiveDateTime::parse_from_str(s, f) {
            return Ok(from_legal_time(t));
        }
    }
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| {
        format!("instant `{s}` (YYYY-MM-DD, YYYY-MM-DDTHH:MM heure légale, ou RFC 3339)")
    })?;
    Ok(from_legal_time(d.and_hms_opt(12, 0, 0).expect("noon")))
}

/// C8: zones and rules at a position and an instant, from the site's `zones` layer.
pub fn zones_at(
    layout: &DataLayout,
    site: &Site,
    lat: f64,
    lon: f64,
    at: DateTime<Utc>,
) -> Result<Evaluation> {
    let dir = layout.layer_dir(site, skimap_zones::LAYER)?;
    let read = |name: &str| {
        let p = dir.join(name);
        std::fs::read_to_string(&p).with_context(|| {
            format!(
                "{} absent : construire d'abord la couche (`skimap zones {}`)",
                p.display(),
                site.id
            )
        })
    };
    let layer = skimap_zones::ZoneLayer::from_geojson_str(&read("zones.geojson")?)?;
    let rules = skimap_zones::RuleSet::from_json_str(&read("rules.json")?)?;
    let position = skimap_common::geom::LonLat::new(lon, lat);
    if !site.bbox.contains(position) {
        bail!(
            "{lat}, {lon} est hors de l'emprise de {} ({})",
            site.id,
            site.bbox.overpass()
        );
    }
    Ok(evaluate(&layer, &rules, position, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants() {
        // Winter: CET, UTC+1.
        assert_eq!(
            parse_instant("2027-01-10").unwrap().to_rfc3339(),
            "2027-01-10T11:00:00+00:00"
        );
        // Summer: CEST, UTC+2.
        assert_eq!(
            parse_instant("2027-07-10T08:30").unwrap().to_rfc3339(),
            "2027-07-10T06:30:00+00:00"
        );
        assert_eq!(
            parse_instant("2027-07-10 08:30").unwrap(),
            parse_instant("2027-07-10T06:30:00Z").unwrap()
        );
        // Last Sunday of March 2027 is the 28th: 03:30 legal time is after the change.
        assert_eq!(
            parse_instant("2027-03-28T03:30").unwrap().to_rfc3339(),
            "2027-03-28T01:30:00+00:00"
        );
        assert!(parse_instant("demain").is_err());
    }

    #[test]
    fn releases_and_dates() {
        let r = parse_release("2026_T2").unwrap();
        assert_eq!((r.year, r.quarter), (2026, 2));
        assert!(parse_release("2026_T5").is_err());
        assert!(parse_release("2026-2").is_err());
        assert_eq!(
            parse_date("2027-01-10").unwrap(),
            NaiveDate::from_ymd_opt(2027, 1, 10).unwrap()
        );
        assert!(parse_date("10/01/2027").is_err());
    }

    #[test]
    fn zones_at_needs_the_layer() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path());
        let at = parse_instant("2027-01-10").unwrap();
        let e = zones_at(&layout, &skimap_common::site::CERVIERES, 44.85, 6.77, at).unwrap_err();
        assert!(format!("{e:#}").contains("skimap zones cervieres"), "{e:#}");
    }
}
