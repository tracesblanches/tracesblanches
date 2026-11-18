//! Builders of the pipeline layers, wired to the public functions of their crates, and
//! `build-all`, which runs them in dependency order with the time of each step.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
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
        let loaded;
        let p = match planet {
            Some(p) => p,
            None => {
                loaded = skimap_osm::extract::load_planet(&self.dl)?;
                &loaded
            }
        };
        Ok(skimap_osm::extract::build_from_with(
            site,
            &self.layout,
            p,
            self.cfg.personal_sources,
        )?)
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

    /// G5: SRN areas of France outside groomed domains, `data/out/france/srn/` (needs the
    /// census, `data/out/france/domaines/`).
    pub fn srn(&self, planet: &Planet) -> Result<skimap_srn::SrnOutput> {
        Ok(skimap_srn::build_from(&self.layout, &self.dl, planet)?)
    }

    /// H1, H2: station cards and access titles of France, `data/out/france/stations/`
    /// (needs the census): the parts that are due now, every part with `force`.
    /// Then the trail starts of every domain (X9) when the cards ran.
    pub fn stations(&self, force: bool) -> Result<skimap_stations::RefreshReport> {
        let r = skimap_stations::refresh(
            &self.layout,
            &self.dl,
            self.cfg.personal_sources,
            Utc::now(),
            force,
        )?;
        crate::france::departs_after_stations(self, &r.ran)?;
        Ok(r)
    }

    /// P5: terrain layer.
    pub fn terrain(&self, site: &Site) -> Result<skimap_terrain::TerrainBuild> {
        Ok(skimap_terrain::build(site, &self.layout, &self.dl)?)
    }

    /// P6: snow layer.
    pub fn snow(&self, site: &Site, date: Option<NaiveDate>) -> Result<skimap_snow::SnowBuild> {
        Ok(skimap_snow::build(site, &self.layout, &self.dl, date)?)
    }

    /// P11, H1: huts and shelters, rebuilt now through the scheduled refresh so that the
    /// cards keep their `changed_at` and the changes reach `journal.jsonl` (a bare
    /// `skimap_refuges::build` would reset them). Key figures of the build.
    pub fn refuges(&self, site: &Site) -> Result<serde_json::Value> {
        let r = skimap_refuges::refresh(site, &self.layout, &self.dl, Utc::now(), true)?;
        if let Some(e) = r.error {
            bail!("{e} (fichiers précédents gardés)");
        }
        Ok(r.stats)
    }

    /// P9, P10: zones and rules.
    pub fn zones(&self, site: &Site) -> Result<skimap_zones::ZonesReport> {
        Ok(skimap_zones::build_with(
            site,
            &self.layout,
            &self.dl,
            self.cfg.personal_sources,
        )?)
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

    /// P19 / X17: hourly weather, Open-Meteo with `personal_sources` (personal use only),
    /// else Météo-France AROME 0.025° (Licence Ouverte), the same layer.
    pub fn meteo(&self, site: &Site) -> Result<skimap_meteo::hourly::HourlyFile> {
        Ok(if self.cfg.personal_sources {
            skimap_meteo::hourly::build(site, &self.layout, &self.dl, true)?
        } else {
            skimap_meteo::arome::build_run(site, &self.layout, &self.dl, self.arome_run()?)?
        })
    }

    /// The AROME run replayed instead of the latest (`SKIMAP_AROME_RUN`), if any.
    pub fn arome_run(&self) -> Result<Option<DateTime<Utc>>> {
        self.cfg
            .arome_run
            .as_deref()
            .map(|r| parse_instant(r).context("SKIMAP_AROME_RUN"))
            .transpose()
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
            Some(d) => skimap_avalanche::historic::build_from_dump_with(
                site,
                &self.layout,
                &self.dl,
                d,
                self.cfg.personal_sources,
            )?,
            None => skimap_avalanche::historic::build_with(
                site,
                &self.layout,
                &self.dl,
                self.cfg.personal_sources,
            )?,
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
        Ok(skimap_avalanche::exposure::build_with(
            site,
            &self.layout,
            &self.dl,
            self.cfg.personal_sources,
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
    /// `refuges`, `zones`, `plan_pistes`), split by provenance by `skimap_voice` (the clips
    /// of personal-use names are listed in `personal.json`, left out of a published pack);
    /// Pocket TTS on this workstation (`VoiceConfig::from_env_lang`), in `lang` (VO6).
    pub fn voice(
        &self,
        site: &Site,
        lang: skimap_voice::Lang,
    ) -> Result<(skimap_voice::pack::VoiceBuild, skimap_voice::SiteNames)> {
        let names = skimap_voice::site_names(&self.layout, site)?;
        let cfg = skimap_voice::VoiceConfig::from_env_lang(lang)?;
        let b = skimap_voice::build_site(site, &self.layout, &names, &cfg)?;
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

    /// VO6: the voice packs of the site, one per language whose voice layer is built
    /// (`voice/`, `voice-en/`), in `data/out/<site>/voice-pack/<lang>/`, each checked;
    /// published apart from the base pack.
    pub fn build_voice_packs(&self, site: &Site) -> Result<Vec<skimap_pack::build::BuildReport>> {
        let mut out = Vec::new();
        for lang in skimap_voice::Lang::ALL {
            let layer = lang.layer();
            if !self
                .layout
                .out_dir(site)
                .join(&layer)
                .join("manifest.json")
                .is_file()
            {
                continue;
            }
            let r = skimap_pack::build::build_voice_pack(
                site,
                &self.layout,
                &layer,
                lang.code(),
                self.cfg.personal_sources,
            )?;
            let problems = skimap_pack::Pack::open(&r.pack_dir)?.verify()?;
            if !problems.is_empty() {
                bail!(
                    "relecture du pack de voix {} : {}",
                    lang.code(),
                    problems.join(" ; ")
                );
            }
            out.push(r);
        }
        Ok(out)
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

    /// Runs every available builder of `site` in dependency order, writing one line per
    /// step to `log`: time and key figures, « IGNORÉ » when a dependency failed, « sans
    /// objet » when the step does not apply to the site (with the reason), « ignoré » for
    /// the personal-use steps when `personal_sources` is off. `shared` gives the planet
    /// extract and the census when they were read once for several sites.
    pub fn build_all_with(
        &self,
        site: &Site,
        shared: &Shared<'_>,
        log: &mut dyn std::io::Write,
    ) -> BuildOutcome {
        let mut run = Run::new(site, self.layout.out_dir(site), shared, log);
        let (pack_bytes, export_bytes) = self.steps(&mut run, shared);
        let resumed = |n: &str| run.steps.iter().any(|s| s.name == n && s.resumed);
        let (pack_resumed, export_resumed) = (resumed("build-pack"), resumed("export"));
        let previous = shared.resume;
        let mut out = run.finish();
        out.pack_bytes = if pack_resumed {
            previous.and_then(|r| r.pack_bytes)
        } else {
            pack_bytes
        };
        out.export_bytes = if export_resumed {
            previous.and_then(|r| r.export_bytes)
        } else {
            export_bytes
        };
        out
    }

    /// The steps of [`Ctx::build_all_with`], with the sizes of the pack and the export
    /// they built. Nothing here names the site: each step is handed the view of it that
    /// holds only the fields of the registry it declares ([`reads`]), the fields its
    /// inputs fingerprint is made of ([`step_inputs`]), so that no step reads a field
    /// whose change would not run it again (#164).
    fn steps(&self, run: &mut Run<'_>, shared: &Shared<'_>) -> (Option<u64>, Option<u64>) {
        // The planet extract, the census and the SRN areas of France: read when a step
        // that needs them runs (`pistes`, `domaines`, `srn`), once for the batch.
        let own_france;
        let france = match shared.france {
            Some(f) => f,
            None => {
                own_france = France::new(self);
                &own_france
            }
        };
        run.step("planet", &[], |_| {
            Ok(if shared.france.is_some() {
                "extrait OSM partagé par les sites, lu à la demande".to_owned()
            } else {
                "extrait OSM lu à la demande".to_owned()
            })
        });
        let planet_ref = || france.planet();
        run.step("domaines", &["planet"], |site| {
            let p = planet_ref()?;
            let census = france.census()?;
            let dir = self.domaines_site(site, p, census)?;
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
        run.step("srn", &["domaines"], |site| {
            let srn = france.srn()?;
            let count =
                |k: skimap_srn::areas::Kind| srn.areas.iter().filter(|a| a.kind == k).count();
            let (areas, routes) = crate::france::write_site_srn(&self.layout, site)?;
            Ok(format!(
                "site : {areas} zones et {routes} itinéraires SRN dans l'emprise ; France : {} zones SRN hors stations ({} domaines de jeu, {} traversées){}",
                srn.areas.len(),
                count(skimap_srn::areas::Kind::Domaine),
                count(skimap_srn::areas::Kind::Traversee),
                if shared.france.is_some() {
                    ", calculées une fois pour tous les sites"
                } else {
                    ""
                }
            ))
        });
        // The graph of an SRN site takes its itineraries (#168): after the `srn` step.
        let pistes_deps: &[&'static str] =
            if skimap_osm::srn_ways::takes_itineraries(&run.view("pistes")) {
                &["planet", "srn"]
            } else {
                &["planet"]
            };
        let mut graph_edges = None;
        if run.resumable("pistes", pistes_deps).is_some() {
            // Taken over: the edge count of the routing test comes from the layer.
            graph_edges = LayerMeta::read(&run.out_dir.join("pistes").join("pistes.meta.json"))
                .ok()
                .and_then(|m| m.stats["graph_edges"].as_u64())
                .map(|n| n as usize);
        }
        run.step("pistes", pistes_deps, |site| {
            let s = self.pistes(site, Some(planet_ref()?))?.stats;
            graph_edges = Some(s.graph_edges);
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
        // The stations layer of France (H1, H2) is not a step of the site: `build-all`
        // refreshes it before reading the inputs of its sites (it rewrites their starts,
        // X9; #164).
        run.step("terrain", &[], |site| {
            let t = self.terrain(site)?;
            let r07 = t.report07.as_ref().map_or_else(String::new, |r| {
                format!(
                    " (rapport 07 : {:.1} %, même emprise : {:.1} %)",
                    r.report * 100.0,
                    r.ours * 100.0
                )
            });
            Ok(format!(
                "{} dalles, {} x {} cellules, {:.1} % valides, altitudes {:.0} à {:.0} m, pente ≥ 30° : {:.1} %{r07}",
                t.tiles,
                t.size_cells[0],
                t.size_cells[1],
                t.valid_fraction * 100.0,
                t.elevation_range_m[0],
                t.elevation_range_m[1],
                t.steep30_fraction * 100.0
            ))
        });
        run.step("snow", &["terrain"], |site| {
            if site.massifs.is_empty() {
                return not_applicable(
                    "hors des massifs de la simulation Météo-France (polygones snowtools)",
                );
            }
            let s = self.snow(site, None)?;
            let [mean, min, max] = s.depth_mean_min_max_cm;
            Ok(format!(
                "simulation du {}, {} cellules estimées, hauteur moyenne {mean:.1} cm (min {min:.0}, max {max:.0})",
                s.date, s.cells_estimated
            ))
        });
        run.step("refuges", &[], |site| {
            let st = self.refuges(site)?;
            Ok(format!(
                "refuges.info {} points, OSM {} éléments, Camptocamp {} dans l'emprise, {} lieux reliés",
                st["refuges_info"]["points"],
                st["osm"]["elements"],
                st["camptocamp"]["in_box"],
                st["links"]["places"]
            ))
        });
        run.step("zones", &[], |site| {
            let z = self.zones(site)?;
            let kinds: Vec<String> = z.by_kind.iter().map(|(k, n)| format!("{k} {n}")).collect();
            Ok(format!("{} zones ({})", z.zones, kinds.join(", ")))
        });
        run.step("avalanche-exposure", &["terrain", "pistes"], |site| {
            let relief = crate::relief::steep_relief(&self.layout, site)?;
            if !relief.avalanche_terrain() {
                return not_applicable(&format!(
                    "relief sous le seuil : {}",
                    relief.describe()
                ));
            }
            let e = self.avalanche_exposure(site)?;
            let [low, exp, high] = e.class_shares;
            Ok(format!(
                "{} x {} cellules de {} m ; terrain peu exposé (modèle) {:.1} %, exposé {:.1} %, très exposé {:.1} % ; CLPA : {} cellules relevées ; {}",
                e.grid.0,
                e.grid.1,
                e.grid.2,
                low * 100.0,
                exp * 100.0,
                high * 100.0,
                e.clpa_cells_raised,
                relief.describe()
            ))
        });
        run.step("routing", &["pistes", "terrain"], |site| {
            if graph_edges == Some(0) {
                return not_applicable("aucune piste dans l'emprise");
            }
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
            |site| {
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
        run.step("coverage", &[], |site| {
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
        run.step("bera", &[], |site| {
            // The BERA layer gives the bulletin of a BERA massif, or the notice and links of
            // the Jura (no BERA there); elsewhere (Vosges, Massif central…) nothing applies.
            if !site.has_bera && !is_jura(site) {
                return not_applicable(&format!(
                    "pas de BERA pour {} (BERA : Alpes, Pyrénées, Corse ; notice sans BERA : Jura)",
                    site.main_massif()
                        .map_or("ce site hors massif".to_owned(), |m| format!(
                            "le massif {} {}",
                            m.code, m.name
                        ))
                ));
            }
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
        // Public mode: AROME (Licence Ouverte); the assistant then gets its weather too.
        run.step("meteo", &[], |site| {
            let h = self.meteo(site)?;
            let hours: usize = h.points.iter().map(|p| p.hours.len()).sum();
            let first = h.points.first();
            Ok(format!(
                "{}{} points, {hours} heures, du {} au {}",
                if self.cfg.personal_sources {
                    ""
                } else {
                    "AROME : "
                },
                h.points.len(),
                first
                    .and_then(|p| p.hours.first())
                    .map_or("?", |x| x.time.as_str()),
                first
                    .and_then(|p| p.hours.last())
                    .map_or("?", |x| x.time.as_str())
            ))
        });
        if self.cfg.personal_sources {
            run.step("status-poll", &["pistes"], |site| {
                if !site.kind.has_groomed_trails() {
                    return not_applicable(&format!(
                        "{} : pas d'état des pistes ni de damage",
                        site.kind.label_fr()
                    ));
                }
                if site.status.is_empty() {
                    return not_applicable(
                        "aucune source d'état lisible pour ce domaine (rapport 08 : widget Nordic France ou ville ENJ)",
                    );
                }
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
                "status-poll",
                "personal_sources désactivé (SKIMAP_PERSONAL_SOURCES)",
            );
        }
        run.step("avalanche-historic", &["pistes"], |site| {
            if site.department != skimap_avalanche::historic::DDT05_DEPARTMENT {
                return not_applicable(&format!(
                    "emprises CLPA publiées seulement pour les Hautes-Alpes (DDT 05) ; département {}",
                    site.department
                ));
            }
            if !self.cfg.personal_sources {
                return not_applicable(
                    "CLPA à usage personnel seulement (personal_sources désactivé)",
                );
            }
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
        run.step("assist", &[], |site| {
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
        run.step("plan", &["routing"], |site| {
            if skimap_plan::presets::archived_day(site).is_none() {
                return not_applicable(
                    "aucun jour archivé pour ce site (table des sites ; `skimap history`, usage personnel)",
                );
            }
            let history = self.layout.layer_dir(site, "assist")?.join("history");
            if !history.is_dir() {
                return not_applicable(&format!(
                    "jour archivé absent : {} (`skimap history`, météo à usage personnel)",
                    history.display()
                ));
            }
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
        let mut pack_bytes = None;
        run.step("build-pack", &[], |site| {
            let r = self.build_pack(site)?;
            let problems = skimap_pack::Pack::open(&r.pack_dir)?.verify()?;
            pack_bytes = Some(r.total_bytes);
            if !problems.is_empty() {
                bail!("relecture du pack : {}", problems.join(" ; "));
            }
            self.build_voice_packs(site)?;
            let mut slow: Vec<&(String, f64)> = r.timings.iter().filter(|t| t.1 >= 1.0).collect();
            slow.sort_by(|a, b| b.1.total_cmp(&a.1));
            let slow: Vec<String> = slow.iter().map(|(k, s)| format!("{k} {s:.1} s")).collect();
            Ok(format!(
                "pack {} : {:.2} Mo, {} couches, {} absentes ; relecture : OK ; {}{}",
                r.pack_version,
                r.total_bytes as f64 / 1e6,
                r.layers.len(),
                r.missing.len(),
                if r.reused.is_empty() {
                    String::new()
                } else {
                    format!(
                        "repris du pack précédent (entrées inchangées) : {} ; ",
                        r.reused.join(", ")
                    )
                },
                if slow.is_empty() {
                    "toutes les parties en moins de 1 s".to_owned()
                } else {
                    slow.join(", ")
                }
            ))
        });
        let mut export_bytes = None;
        run.step("export", &["build-pack"], |site| {
            let r = self.export(site)?;
            export_bytes = Some(r.total_bytes);
            Ok(format!(
                "{} fichiers, {:.2} Mo, {} couches non exportées",
                r.files.len(),
                r.total_bytes as f64 / 1e6,
                r.skipped.len()
            ))
        });
        (pack_bytes, export_bytes)
    }
}

/// Writes the steps done so far (journal of a site build).
pub type Journal<'a> = dyn Fn(&[StepRecord]) + Sync + 'a;

/// France-wide inputs of the `build-all` of several sites (planet extract, census, SRN
/// areas), read once and only when a step of some site needs them (#164): a pass whose
/// sites only need their weather refreshed never reads the extract (80 s on the
/// workstation).
pub struct France<'c> {
    ctx: &'c Ctx,
    planet: std::sync::OnceLock<std::result::Result<Planet, String>>,
    census: std::sync::OnceLock<std::result::Result<CensusOutput, String>>,
    srn: std::sync::OnceLock<std::result::Result<skimap_srn::SrnOutput, String>>,
}

impl<'c> France<'c> {
    /// Nothing read yet.
    pub fn new(ctx: &'c Ctx) -> Self {
        Self {
            ctx,
            planet: std::sync::OnceLock::new(),
            census: std::sync::OnceLock::new(),
            srn: std::sync::OnceLock::new(),
        }
    }

    /// The planet extract, read on first use.
    pub fn planet(&self) -> Result<&Planet> {
        self.planet
            .get_or_init(|| {
                let t = Instant::now();
                let p =
                    skimap_osm::extract::load_planet(&self.ctx.dl).map_err(|e| format!("{e:#}"))?;
                println!(
                    "extrait OSM du {} lu en {:.1} s",
                    p.valid_at.as_deref().unwrap_or("?"),
                    t.elapsed().as_secs_f64()
                );
                Ok(p)
            })
            .as_ref()
            .map_err(|e| anyhow!("extrait OSM : {e}"))
    }

    /// The census of France, made on first use.
    pub fn census(&self) -> Result<&CensusOutput> {
        self.census
            .get_or_init(|| {
                let t = Instant::now();
                let p = self.planet().map_err(|e| format!("{e:#}"))?;
                let c = self.ctx.census(p).map_err(|e| format!("{e:#}"))?;
                println!(
                    "recensement ({} domaines) fait en {:.1} s",
                    c.domains.len(),
                    t.elapsed().as_secs_f64()
                );
                Ok(c)
            })
            .as_ref()
            .map_err(|e| anyhow!("recensement : {e}"))
    }

    /// The SRN areas of France, made on first use.
    pub fn srn(&self) -> Result<&skimap_srn::SrnOutput> {
        self.srn
            .get_or_init(|| {
                let p = self.planet().map_err(|e| format!("{e:#}"))?;
                let s = self.ctx.srn(p).map_err(|e| format!("{e:#}"))?;
                println!("{} zones SRN hors stations", s.areas.len());
                Ok(s)
            })
            .as_ref()
            .map_err(|e| anyhow!("zones SRN : {e}"))
    }
}

/// Inputs read once and shared by the `build-all` of several sites.
#[derive(Default, Clone, Copy)]
pub struct Shared<'a> {
    /// France-wide inputs, read on demand once for every site of a batch.
    pub france: Option<&'a France<'a>>,
    /// Steps taken over from the previous run of the site.
    pub resume: Option<Resume<'a>>,
    /// Called with the steps so far after every step (journal of the site, so that an
    /// interrupted run is taken over where it stopped).
    pub journal: Option<&'a Journal<'a>>,
}

/// French line of a refresh of the stations layer: parts run, cards, changes, failures.
pub fn describe_stations(r: &skimap_stations::RefreshReport) -> String {
    let list = |v: &[String]| {
        if v.is_empty() {
            "aucune".to_owned()
        } else {
            v.join(", ")
        }
    };
    let mut s = format!(
        "France : {} fiches de domaine ; parties refaites : {} ; à jour : {} ; {} changements",
        r.stats["domains"],
        list(&r.ran),
        list(&r.skipped),
        r.changes
    );
    if let Some(n) = r.stats["personal"]["titres"].as_object().map(|t| t.len()) {
        s.push_str(&format!(" ; {n} titres (usage personnel)"));
    }
    if !r.broken_sources.is_empty() {
        s.push_str(&format!(
            " ; sources en échec : {}",
            r.broken_sources.join(", ")
        ));
    }
    if !r.stale_titres.is_empty() {
        s.push_str(&format!(
            " ; titres non relus (dernière lecture réussie gardée, avec sa date) : {}",
            r.stale_titres.join(", ")
        ));
    }
    s
}

/// A step that does not apply to the site (reason in French).
#[derive(Debug)]
pub struct NotApplicable(pub String);

impl std::fmt::Display for NotApplicable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotApplicable {}

fn not_applicable(why: &str) -> Result<String> {
    Err(NotApplicable(why.to_owned()).into())
}

/// Whether the main massif of a site is in the Jura (snowtools 55 to 58), where the BERA
/// layer gives the notice and links of a massif without bulletin.
fn is_jura(site: &Site) -> bool {
    site.main_massif()
        .is_some_and(|m| (55..=58).contains(&m.code))
}

/// State of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Done.
    Ok,
    /// Failed.
    Failed,
    /// Not run: a dependency failed.
    Blocked,
    /// Does not apply to the site, or a dependency does not.
    NotApplicable,
    /// Personal-use step with `personal_sources` off.
    Disabled,
}

/// One step of a `build-all` run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    /// Step name.
    pub name: String,
    /// State.
    pub state: StepState,
    /// Seconds.
    pub seconds: f64,
    /// Key figures, error or reason.
    pub line: String,
    /// When the outputs of the step were produced (RFC 3339): the time of the run that
    /// computed them, kept when a later run takes them over ([`Resume`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Fingerprint of the step's output directory ([`fingerprint`]) after the step: a
    /// later run takes the step over only if the directory still has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<String>,
    /// Taken over from the previous run, not run again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resumed: bool,
    /// Fingerprint of the fields of the site record the step read ([`step_inputs`]): a
    /// later run takes the step over only with the same (`None`: not run, or recorded
    /// before #164, then that of the record of the previous build).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<String>,
}

/// A field of the site record that a step reads ([`Site`]; the census details that
/// `Site` does not carry — census identifier, commune name, envelope, km, sections,
/// status source of report 08 — are read by no step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// [`Site::name`].
    Name,
    /// [`Site::department`].
    Department,
    /// [`Site::insee`].
    Insee,
    /// [`Site::commune_center`].
    CommuneCenter,
    /// [`Site::bbox`] (the working area).
    Bbox,
    /// [`Site::massifs`].
    Massifs,
    /// [`Site::massif_status`].
    MassifStatus,
    /// [`Site::has_bera`].
    HasBera,
    /// [`Site::kind`].
    Kind,
    /// [`Site::parent`].
    Parent,
    /// [`Site::status`]: trail status sources (personal use).
    Status,
    /// [`Site::starts`]: trail starts (X9), rewritten by the stations step.
    Starts,
}

/// The place of the site: every field but the status sources and the starts.
const PLACE: &[Field] = &[
    Field::Name,
    Field::Department,
    Field::Insee,
    Field::CommuneCenter,
    Field::Bbox,
    Field::Massifs,
    Field::MassifStatus,
    Field::HasBera,
    Field::Kind,
    Field::Parent,
];
/// The place and the starts.
const PLACE_STARTS: &[Field] = &[
    Field::Name,
    Field::Department,
    Field::Insee,
    Field::CommuneCenter,
    Field::Bbox,
    Field::Massifs,
    Field::MassifStatus,
    Field::HasBera,
    Field::Kind,
    Field::Parent,
    Field::Starts,
];
/// Every field.
const ALL_FIELDS: &[Field] = &[
    Field::Name,
    Field::Department,
    Field::Insee,
    Field::CommuneCenter,
    Field::Bbox,
    Field::Massifs,
    Field::MassifStatus,
    Field::HasBera,
    Field::Kind,
    Field::Parent,
    Field::Status,
    Field::Starts,
];

/// Fields of the site record each step of [`Ctx::build_all_with`] reads (the view it is
/// handed, [`view`]). The starts (X9) are read by the weather (`skimap_meteo::hourly`:
/// one forecast point per start), the assistant (`skimap_assist::presets`), the day
/// plans (`skimap_plan::presets`), the pack (`skimap_pack::build::starts`, the site
/// record of the pack) and the export (from the pack); the status sources by the
/// polling of the trail status only. The place is declared by every step that reads
/// the site.
pub const READS: &[(&str, &[Field])] = &[
    ("planet", &[]),
    ("domaines", PLACE),
    ("srn", PLACE),
    ("pistes", PLACE),
    ("terrain", PLACE),
    ("snow", PLACE),
    ("refuges", PLACE),
    ("zones", PLACE),
    ("avalanche-exposure", PLACE),
    ("routing", PLACE),
    ("offpiste-prepare", PLACE),
    ("coverage", PLACE),
    ("bera", PLACE),
    ("meteo", PLACE_STARTS),
    (
        "status-poll",
        &[
            Field::Name,
            Field::Department,
            Field::Insee,
            Field::CommuneCenter,
            Field::Bbox,
            Field::Massifs,
            Field::MassifStatus,
            Field::HasBera,
            Field::Kind,
            Field::Parent,
            Field::Status,
        ],
    ),
    ("avalanche-historic", PLACE),
    ("assist", PLACE_STARTS),
    ("plan", PLACE_STARTS),
    ("build-pack", PLACE_STARTS),
    ("export", PLACE_STARTS),
];

/// Fields of the site record `step` reads ([`READS`]; every field for a step not listed).
pub fn reads(step: &str) -> &'static [Field] {
    READS
        .iter()
        .find(|(n, _)| *n == step)
        .map_or(ALL_FIELDS, |(_, f)| f)
}

/// `site` with only `fields`: the others empty (the identifier is always kept: the
/// paths of the site).
pub fn view(site: &Site, fields: &[Field]) -> Site {
    let has = |f: Field| fields.contains(&f);
    Site {
        id: site.id,
        name: if has(Field::Name) { site.name } else { "" },
        department: if has(Field::Department) {
            site.department
        } else {
            ""
        },
        insee: if has(Field::Insee) { site.insee } else { "" },
        commune_center: if has(Field::CommuneCenter) {
            site.commune_center
        } else {
            skimap_common::geom::LonLat { lon: 0.0, lat: 0.0 }
        },
        bbox: if has(Field::Bbox) {
            site.bbox
        } else {
            skimap_common::geom::BBox {
                south: 0.0,
                west: 0.0,
                north: 0.0,
                east: 0.0,
            }
        },
        massifs: if has(Field::Massifs) {
            site.massifs
        } else {
            &[]
        },
        massif_status: if has(Field::MassifStatus) {
            site.massif_status
        } else {
            skimap_common::site::MassifStatus::Outside
        },
        has_bera: has(Field::HasBera) && site.has_bera,
        kind: if has(Field::Kind) {
            site.kind
        } else {
            skimap_common::site::SiteKind::default()
        },
        parent: if has(Field::Parent) {
            site.parent
        } else {
            None
        },
        status: if has(Field::Status) {
            site.status
        } else {
            skimap_common::site::StatusSources::default()
        },
        starts: if has(Field::Starts) { site.starts } else { &[] },
    }
}

/// Fingerprint of the fields of `site` that `step` reads ([`reads`], [`view`]).
pub fn step_inputs(step: &str, site: &Site) -> String {
    use sha2::{Digest, Sha256};
    let v = view(site, reads(step));
    let body = serde_json::to_vec(&v).expect("a site serializes");
    hex::encode(Sha256::digest(body))
}

/// Whether a step is run again once older than the age limit of [`Resume`] (#172): the
/// steps that read an external source whose changes leave no local fingerprint, so that
/// only time can tell them. The other steps read only local outputs (of the steps they
/// depend on, whose changes run them again) and the site fields they declare
/// ([`step_inputs`]): their age alone never runs them again.
///
/// | Step | Aged | External source |
/// |---|---|---|
/// | `domaines`, `srn`, `pistes` | yes | planet extract (Protomaps build), census, SRN sources |
/// | `terrain` | yes | IGN RGE ALTI / LiDAR HD tiles (new tiles, new flights) |
/// | `snow`, `bera`, `meteo`, `status-poll` | yes | daily Météo-France, trail status |
/// | `refuges`, `zones`, `coverage`, `avalanche-historic` | yes | refuges.info / OSM / Camptocamp, protected areas, ARCEP release, CLPA |
/// | `build-pack` | yes | Protomaps basemap and Plan IGN (renewed at most weekly inside the pack) |
/// | `avalanche-exposure` | personal sources only | LiDAR HD canopy tiles of the terrain tiles (follow `terrain`), CLPA (personal use) |
/// | `routing`, `offpiste-prepare`, `export` | no | local outputs only |
pub fn ages(step: &str, personal_sources: bool) -> bool {
    match step {
        "routing" | "offpiste-prepare" | "export" => false,
        "avalanche-exposure" => personal_sources,
        _ => true,
    }
}

/// Output directory (layer) of a step that can be taken over by a later run: its
/// outputs are all in that directory of the site.
pub fn step_dir(step: &str) -> Option<&'static str> {
    Some(match step {
        "pistes" => "pistes",
        "domaines" => "domaines",
        "srn" => "srn",
        "terrain" => "terrain",
        "snow" => "snow",
        "refuges" => "refuges",
        "zones" => "zones",
        "avalanche-exposure" => "avalanche-exposure",
        "routing" => "routes",
        "offpiste-prepare" => "offpiste",
        "coverage" => "coverage",
        "bera" => "bera",
        "meteo" => "meteo-hourly",
        "avalanche-historic" => "avalanche-historic",
        "build-pack" => "pack",
        "export" => "export",
        _ => return None,
    })
}

/// Number of features of a GeoJSON file, without keeping them; `Err` when the file is
/// missing or is not a feature collection.
fn feature_count(path: &Path) -> std::result::Result<usize, String> {
    #[derive(Deserialize)]
    struct Fc {
        features: Vec<serde::de::IgnoredAny>,
    }
    let body = std::fs::read(path).map_err(|e| format!("{} : {e}", path.display()))?;
    serde_json::from_slice::<Fc>(&body)
        .map(|f| f.features.len())
        .map_err(|e| format!("{} illisible : {e}", path.display()))
}

/// Whether a directory holds at least one file (temporary files left out).
fn has_files(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            has_files(&e.path())
        } else {
            !e.file_name().to_string_lossy().contains(".tmp")
        }
    })
}

/// Why a step that ended `ok` produced nothing usable, from its outputs on disk in the
/// output directory of the site (`site_dir`); `None` when it produced something (B167:
/// an empty layer is reported in the build log and in `status.json`, never silently).
/// Empty means: no output file at all; no piste; no hut or shelter from any of the three
/// sources; no zone (the country and the department are always there); no pack manifest.
pub fn empty_output(step: &str, site_dir: &Path, site: &Site) -> Option<String> {
    let dir_name = step_dir(step)?;
    let dir = site_dir.join(dir_name);
    if !has_files(&dir) {
        return Some(format!("aucun fichier écrit dans {dir_name}/"));
    }
    let count = |files: &[&str]| -> std::result::Result<usize, String> {
        files
            .iter()
            .map(|f| feature_count(&dir.join(f)))
            .sum::<std::result::Result<usize, String>>()
    };
    let (files, nothing): (&[&str], String) = match step {
        "pistes" => (
            &["pistes.geojson"],
            if site.kind.has_groomed_trails() {
                "aucune piste OSM dans l'emprise d'une station damée : emprise du registre ou extrait OSM en défaut".into()
            } else {
                format!(
                    "aucune piste damée dans l'emprise ({} : itinéraires hors piste seulement)",
                    site.kind.label_fr()
                )
            },
        ),
        "refuges" => (
            &["refuges_info.geojson", "osm.geojson", "camptocamp.geojson"],
            "aucun refuge ni abri dans l'emprise (refuges.info, OSM, Camptocamp)".into(),
        ),
        "zones" => (
            &["zones.geojson"],
            "aucune zone, pas même le pays et le département".into(),
        ),
        "build-pack" => {
            return (!dir.join("manifest.json").is_file())
                .then(|| "pack sans manifest.json".to_owned());
        }
        _ => return None,
    };
    match count(files) {
        Ok(0) => Some(nothing),
        Ok(_) => None,
        Err(e) => Some(e),
    }
}

/// Fingerprint of a directory: sha256 over its files (relative path, size, sha256 of
/// the content), in path order; temporary files (`*.tmp*`) left out. `None` when the
/// directory does not exist.
pub fn fingerprint(dir: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                walk(base, &p, out);
            } else {
                let rel = p
                    .strip_prefix(base)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .into_owned();
                if !rel.contains(".tmp") {
                    out.push((rel, p));
                }
            }
        }
    }
    if !dir.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    let mut h = Sha256::new();
    for (rel, p) in files {
        let mut fh = Sha256::new();
        let mut f = std::fs::File::open(&p).ok()?;
        let mut buf = vec![0u8; 1 << 16];
        let mut n = 0u64;
        loop {
            let k = std::io::Read::read(&mut f, &mut buf).ok()?;
            if k == 0 {
                break;
            }
            fh.update(&buf[..k]);
            n += k as u64;
        }
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(n.to_le_bytes());
        h.update(fh.finalize());
    }
    Some(hex::encode(h.finalize()))
}

/// What a run takes over from the previous run of the site (`build-all.json`): a step
/// whose output directory still has its fingerprint, whose inputs did not change and
/// whose dependencies did not change in this run is not run again, unless it reads an
/// external source without local fingerprint ([`ages`]) and was done `max_age` ago. The pack and the
/// export are rebuilt as soon as any step changed its outputs.
#[derive(Clone, Copy)]
pub struct Resume<'a> {
    /// Steps of the previous run (or of the interrupted one: the journal is written after
    /// every step).
    pub previous: &'a [StepRecord],
    /// Age beyond which a step reading an external source is run again ([`ages`]).
    pub max_age: std::time::Duration,
    /// Personal-use sources on (CLPA in the exposure to avalanches, [`ages`]).
    pub personal_sources: bool,
    /// Pack and export sizes of the previous run.
    pub pack_bytes: Option<u64>,
    /// Export size of the previous run.
    pub export_bytes: Option<u64>,
}

/// Result of the `build-all` of one site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutcome {
    /// Site.
    pub site: String,
    /// Steps in order.
    pub steps: Vec<StepRecord>,
    /// Total seconds.
    pub seconds: f64,
    /// Pack size, bytes.
    pub pack_bytes: Option<u64>,
    /// Export size, bytes.
    pub export_bytes: Option<u64>,
}

impl BuildOutcome {
    /// Steps that failed or were blocked.
    pub fn failed(&self) -> Vec<&StepRecord> {
        self.steps
            .iter()
            .filter(|s| matches!(s.state, StepState::Failed | StepState::Blocked))
            .collect()
    }

    /// Whether every step succeeded, did not apply or was disabled.
    pub fn ok(&self) -> bool {
        self.failed().is_empty()
    }

    /// `Ok` when [`BuildOutcome::ok`], else the failed steps.
    pub fn into_result(self) -> Result<()> {
        if self.ok() {
            Ok(())
        } else {
            bail!(
                "build-all {} : étapes en échec ou ignorées : {}",
                self.site,
                self.failed()
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

/// Steps of a `build-all` run.
struct Run<'a> {
    site: &'a Site,
    out_dir: PathBuf,
    t0: Instant,
    failed: BTreeSet<&'static str>,
    not_applicable: BTreeSet<&'static str>,
    /// Steps whose outputs differ from the previous run (run again, failed, or new).
    changed: BTreeSet<&'static str>,
    steps: Vec<StepRecord>,
    resume: Option<Resume<'a>>,
    journal: Option<&'a Journal<'a>>,
    log: &'a mut dyn std::io::Write,
}

impl<'a> Run<'a> {
    fn new(
        site: &'a Site,
        out_dir: PathBuf,
        shared: &Shared<'a>,
        log: &'a mut dyn std::io::Write,
    ) -> Self {
        let _ = writeln!(
            log,
            "build-all {} ({}, {})",
            site.id,
            site.name,
            site.kind.label_fr()
        );
        Self {
            site,
            out_dir,
            t0: Instant::now(),
            failed: BTreeSet::new(),
            not_applicable: BTreeSet::new(),
            changed: BTreeSet::new(),
            steps: Vec::new(),
            resume: shared.resume,
            journal: shared.journal,
            log,
        }
    }

    fn record(&mut self, name: &str, state: StepState, seconds: f64, line: String) {
        self.record_full(StepRecord {
            name: name.to_owned(),
            state,
            seconds,
            line,
            at: None,
            outputs: None,
            resumed: false,
            inputs: None,
        });
    }

    /// The site as step `name` sees it ([`view`]).
    fn view(&self, name: &str) -> Site {
        view(self.site, reads(name))
    }

    fn record_full(&mut self, r: StepRecord) {
        let (name, seconds, line) = (&r.name, r.seconds, &r.line);
        let shown = match r.state {
            StepState::Ok if r.resumed => format!(
                "{name:<19} {:>8}  repris (fait le {}) : {line}",
                "-",
                r.at.as_deref().unwrap_or("?")
            ),
            StepState::NotApplicable if r.resumed => {
                format!("{name:<19} {:>8}  sans objet (repris) : {line}", "-")
            }
            StepState::Ok => format!("{name:<19} {seconds:>7.2}s  {line}"),
            StepState::Failed => format!("{name:<19} {seconds:>7.2}s  ÉCHEC : {line}"),
            StepState::Blocked => format!("{name:<19} {:>8}  IGNORÉ : {line}", "-"),
            StepState::NotApplicable => {
                format!("{name:<19} {seconds:>7.2}s  sans objet : {line}")
            }
            StepState::Disabled => format!("{name:<19} {:>8}  ignoré : {line}", "-"),
        };
        let shown = match r.state {
            StepState::Ok => match empty_output(name, &self.out_dir, self.site) {
                Some(why) => format!("{shown}\n{:<19} {:>8}  VIDE : {why}", "", ""),
                None => shown,
            },
            _ => shown,
        };
        let _ = writeln!(self.log, "{shown}");
        let _ = self.log.flush();
        self.steps.push(r);
        if let Some(j) = self.journal {
            j(&self.steps);
        }
    }

    /// The previous record of `name` when the step can be taken over ([`Resume`]).
    fn resumable(&self, name: &str, deps: &[&'static str]) -> Option<StepRecord> {
        let r = self.resume?;
        let dir = step_dir(name)?;
        let prev = r.previous.iter().find(|s| s.name == name)?;
        if !matches!(prev.state, StepState::Ok | StepState::NotApplicable) {
            return None;
        }
        let at = DateTime::parse_from_rfc3339(prev.at.as_deref()?).ok()?;
        let age = (Utc::now() - at.to_utc()).to_std().ok()?;
        if age >= r.max_age && ages(name, r.personal_sources) {
            return None;
        }
        // The fields of the site record the step reads changed (#164): run again. A
        // change of a field it does not read (the starts for the terrain) does not.
        if prev.inputs.as_deref() != Some(step_inputs(name, self.site).as_str()) {
            return None;
        }
        // The pack and the export take every layer: rebuilt when any step changed.
        let deps_changed = if matches!(name, "build-pack" | "export") {
            !self.changed.is_empty()
        } else {
            deps.iter().any(|d| self.changed.contains(d))
        };
        if deps_changed {
            return None;
        }
        // Outputs missing, incomplete or altered since: run again. A step that wrote no
        // directory (`srn` of a site without SRN area) is taken over while it has none.
        if prev.state == StepState::Ok && fingerprint(&self.out_dir.join(dir)) != prev.outputs {
            return None;
        }
        Some(prev.clone())
    }

    fn step(
        &mut self,
        name: &'static str,
        deps: &[&'static str],
        f: impl FnOnce(&Site) -> Result<String>,
    ) {
        if let Some(d) = deps.iter().find(|d| self.failed.contains(*d)) {
            self.failed.insert(name);
            self.record(name, StepState::Blocked, 0.0, format!("{d} a échoué"));
            return;
        }
        if let Some(d) = deps.iter().find(|d| self.not_applicable.contains(*d)) {
            self.not_applicable.insert(name);
            self.record(
                name,
                StepState::NotApplicable,
                0.0,
                format!("dépend de {d}, sans objet"),
            );
            return;
        }
        if let Some(prev) = self.resumable(name, deps) {
            if prev.state == StepState::NotApplicable {
                self.not_applicable.insert(name);
            }
            self.record_full(StepRecord {
                seconds: 0.0,
                resumed: true,
                ..prev
            });
            return;
        }
        let t = Instant::now();
        let site = self.view(name);
        // A panic in a step fails that step only: the other steps of the site and the other
        // sites of the batch go on.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&site))).unwrap_or_else(
            |p| {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "?".to_owned());
                Err(anyhow::anyhow!("panique : {msg}"))
            },
        );
        let s = t.elapsed().as_secs_f64();
        let at = Some(skimap_common::meta::now_rfc3339());
        let previous = self
            .resume
            .and_then(|r| r.previous.iter().find(|p| p.name == name).cloned());
        let (state, line) = match r {
            Ok(line) => (StepState::Ok, line),
            Err(e) => match e.downcast_ref::<NotApplicable>() {
                Some(na) => {
                    self.not_applicable.insert(name);
                    (StepState::NotApplicable, na.0.clone())
                }
                None => {
                    self.failed.insert(name);
                    (StepState::Failed, format!("{e:#}"))
                }
            },
        };
        let outputs = match (state, step_dir(name)) {
            (StepState::Ok, Some(d)) => fingerprint(&self.out_dir.join(d)),
            _ => None,
        };
        let same = previous
            .as_ref()
            .is_some_and(|p| p.state == state && p.outputs == outputs);
        if !same {
            self.changed.insert(name);
        }
        self.record_full(StepRecord {
            name: name.to_owned(),
            state,
            seconds: s,
            line,
            at,
            outputs,
            resumed: false,
            inputs: Some(step_inputs(name, self.site)),
        });
    }

    fn skip(&mut self, name: &'static str, why: &str) {
        self.record(name, StepState::Disabled, 0.0, why.to_owned());
    }

    fn finish(self) -> BuildOutcome {
        let total = self.t0.elapsed().as_secs_f64();
        let peak = peak_rss_mb().map_or_else(String::new, |m| {
            format!("  mémoire maximale du processus {m:.0} Mo")
        });
        let _ = writeln!(self.log, "{:<19} {total:>7.2}s{peak}", "total");
        BuildOutcome {
            site: self.site.id.to_owned(),
            steps: self.steps,
            seconds: total,
            pack_bytes: None,
            export_bytes: None,
        }
    }
}

/// Peak resident memory of the process so far, MB (`VmHWM` of `/proc/self/status`;
/// `None` elsewhere than on Linux). Process-wide: per site only when one site is built.
pub fn peak_rss_mb() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
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

    /// B167: what an `ok` step left on disk says whether it produced anything.
    #[test]
    fn empty_outputs() {
        let d = tempfile::tempdir().unwrap();
        let rec = |groomed: bool| {
            skimap_common::site::evaluation_records()
                .iter()
                .find(|r| r.kind.has_groomed_trails() == groomed)
                .cloned()
        };
        let station = skimap_common::site::interned(&rec(true).unwrap()).unwrap();
        let mut srn = rec(true).unwrap();
        srn.id = format!("{}-srn", srn.id);
        srn.kind = skimap_common::site::SiteKind::SrnArea;
        let srn = skimap_common::site::interned(&srn).unwrap();
        let dir = d.path();
        let fc = |n: usize| {
            serde_json::json!({"type": "FeatureCollection", "features": vec![serde_json::json!({}); n]})
                .to_string()
        };
        // Nothing written at all.
        for step in ["pistes", "terrain", "build-pack"] {
            assert!(
                empty_output(step, dir, station)
                    .unwrap()
                    .starts_with("aucun fichier"),
                "{step}"
            );
        }
        // Steps without an output directory of the site are never judged.
        assert_eq!(empty_output("planet", dir, station), None);
        std::fs::create_dir_all(dir.join("pistes")).unwrap();
        std::fs::write(dir.join("pistes/pistes.geojson"), fc(0)).unwrap();
        assert!(
            empty_output("pistes", dir, station)
                .unwrap()
                .contains("station damée")
        );
        assert!(
            empty_output("pistes", dir, srn)
                .unwrap()
                .contains("hors piste")
        );
        std::fs::write(dir.join("pistes/pistes.geojson"), fc(3)).unwrap();
        assert_eq!(empty_output("pistes", dir, station), None);
        std::fs::write(dir.join("pistes/pistes.geojson"), "{").unwrap();
        assert!(
            empty_output("pistes", dir, station)
                .unwrap()
                .contains("illisible")
        );
        // A pack directory without its manifest.
        std::fs::create_dir_all(dir.join("pack/layers")).unwrap();
        std::fs::write(dir.join("pack/layers/x.pmtiles"), b"x").unwrap();
        assert_eq!(
            empty_output("build-pack", dir, station).as_deref(),
            Some("pack sans manifest.json")
        );
        std::fs::write(dir.join("pack/manifest.json"), b"{}").unwrap();
        assert_eq!(empty_output("build-pack", dir, station), None);
    }

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
        let e = zones_at(
            &layout,
            skimap_common::site::site("cervieres").expect("evaluation site"),
            44.85,
            6.77,
            at,
        )
        .unwrap_err();
        assert!(format!("{e:#}").contains("skimap zones cervieres"), "{e:#}");
    }
}

/// Steps taken over, run again, isolated (journal, fingerprints, panics): `Run` with
/// stand-in steps writing into a temporary site directory.
#[cfg(test)]
mod resume_tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::Mutex;

    /// A run of three stand-in steps: `terrain` writes `terrain/dem`, `refuges` writes
    /// `refuges/r` (or fails, or panics), `build-pack` writes `pack/p` from both.
    fn run(
        dir: &Path,
        resume: Option<Resume<'_>>,
        refuges: &str,
        calls: &Cell<[u32; 3]>,
        journal: &Mutex<Vec<Vec<StepRecord>>>,
    ) -> BuildOutcome {
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let j = |s: &[StepRecord]| journal.lock().unwrap().push(s.to_vec());
        let shared = Shared {
            resume,
            journal: Some(&j),
            ..Shared::default()
        };
        let mut log = Vec::new();
        let mut r = Run::new(site, dir.to_path_buf(), &shared, &mut log);
        let bump = |i: usize| {
            let mut c = calls.get();
            c[i] += 1;
            calls.set(c);
        };
        r.step("terrain", &[], |_| {
            bump(0);
            std::fs::create_dir_all(dir.join("terrain"))?;
            std::fs::write(dir.join("terrain/dem"), b"dem")?;
            Ok("terrain".into())
        });
        r.step("refuges", &[], |_| {
            bump(1);
            match refuges {
                "fail" => bail!("HTTP status 502"),
                "panic" => panic!("boom"),
                v => {
                    std::fs::create_dir_all(dir.join("refuges"))?;
                    std::fs::write(dir.join("refuges/r"), v)?;
                    Ok(format!("refuges {v}"))
                }
            }
        });
        r.step("build-pack", &[], |_| {
            bump(2);
            std::fs::create_dir_all(dir.join("pack"))?;
            let t = std::fs::read(dir.join("terrain/dem"))?;
            let f = std::fs::read(dir.join("refuges/r")).unwrap_or_default();
            std::fs::write(dir.join("pack/p"), [t, f].concat())?;
            Ok("pack".into())
        });
        r.finish()
    }

    fn resume(steps: &[StepRecord]) -> Resume<'_> {
        Resume {
            previous: steps,
            max_age: std::time::Duration::from_secs(3600),
            pack_bytes: None,
            export_bytes: None,
            personal_sources: false,
        }
    }

    #[test]
    fn failed_steps_only_are_run_again_and_the_rest_is_taken_over() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let calls = Cell::new([0; 3]);
        let journal = Mutex::new(Vec::new());
        // Source down: refuges fails, the pack is still built (with what exists).
        let o1 = run(dir, None, "fail", &calls, &journal);
        assert!(!o1.ok());
        assert_eq!(calls.get(), [1, 1, 1]);
        assert!(dir.join("pack/p").exists());
        // Journal written after every step.
        assert_eq!(
            journal
                .lock()
                .unwrap()
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        // Source back: only refuges runs again, then the pack (its input changed).
        let o2 = run(dir, Some(resume(&o1.steps)), "v1", &calls, &journal);
        assert!(o2.ok(), "{:?}", o2.steps);
        assert_eq!(calls.get(), [1, 2, 2]);
        assert!(o2.steps[0].resumed && !o2.steps[1].resumed && !o2.steps[2].resumed);
        assert_eq!(o2.steps[0].at, o1.steps[0].at, "date of the outputs kept");
        // Nothing to do: everything taken over, no step body called (idempotent).
        let o3 = run(dir, Some(resume(&o2.steps)), "v1", &calls, &journal);
        assert!(o3.ok());
        assert_eq!(calls.get(), [1, 2, 2]);
        assert!(o3.steps.iter().all(|s| s.resumed));
        // A corrupted output is detected and rebuilt; rebuilt identical, what depends on
        // it is still valid and taken over.
        std::fs::write(dir.join("terrain/dem"), b"de").unwrap();
        let o4 = run(dir, Some(resume(&o3.steps)), "v1", &calls, &journal);
        assert_eq!(calls.get(), [2, 2, 2]);
        assert!(o4.ok() && o4.steps[2].resumed);
        assert_eq!(std::fs::read(dir.join("pack/p")).unwrap(), b"demv1");
        // A corrupted pack is rebuilt.
        std::fs::write(dir.join("pack/p"), b"x").unwrap();
        let o4 = run(dir, Some(resume(&o4.steps)), "v1", &calls, &journal);
        assert_eq!(calls.get(), [2, 2, 3]);
        assert_eq!(std::fs::read(dir.join("pack/p")).unwrap(), b"demv1");
        // An output removed and rebuilt different: the pack follows.
        std::fs::remove_dir_all(dir.join("refuges")).unwrap();
        let o5 = run(dir, Some(resume(&o4.steps)), "v2", &calls, &journal);
        assert_eq!(calls.get(), [2, 3, 4]);
        assert_eq!(std::fs::read(dir.join("pack/p")).unwrap(), b"demv2");
        // A step run again with identical outputs does not rebuild the pack.
        let mut stale = o5.steps.clone();
        stale[1].at = Some("2000-01-01T00:00:00Z".into());
        let o6 = run(dir, Some(resume(&stale)), "v2", &calls, &journal);
        assert_eq!(
            calls.get(),
            [2, 4, 4],
            "refuges older than max_age: run again"
        );
        assert!(o6.steps[2].resumed);
    }

    /// #172: the age limit of the catch-up (`--max-age-hours 168`) redid every step a
    /// week old, the heavy ones derived from local outputs too (exposure to avalanches,
    /// off-piste rasters: 5.6 GB on a big SRN area, killed on the server), although their
    /// inputs fingerprints had not changed. Only a step reading an external source
    /// without a local fingerprint (the terrain here) is run again by age; derived steps
    /// follow it only when its outputs change.
    #[test]
    fn age_redoes_only_the_steps_of_an_external_source() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let calls = Cell::new([0u32; 3]);
        let go = |resume: Option<Resume<'_>>, dem: &'static [u8]| {
            let shared = Shared {
                resume,
                ..Shared::default()
            };
            let mut log = Vec::new();
            let mut r = Run::new(site, dir.to_path_buf(), &shared, &mut log);
            let bump = |i: usize| {
                let mut c = calls.get();
                c[i] += 1;
                calls.set(c);
            };
            let write = |d: &str, v: &[u8]| -> Result<String> {
                std::fs::create_dir_all(dir.join(d))?;
                std::fs::write(dir.join(d).join("f"), v)?;
                Ok(d.to_owned())
            };
            r.step("terrain", &[], |_| {
                bump(0);
                write("terrain", dem)
            });
            r.step("avalanche-exposure", &["terrain"], |_| {
                bump(1);
                write("avalanche-exposure", &std::fs::read(dir.join("terrain/f"))?)
            });
            r.step("offpiste-prepare", &["terrain"], |_| {
                bump(2);
                write("offpiste", &std::fs::read(dir.join("terrain/f"))?)
            });
            r.finish()
        };
        let o1 = go(None, b"dem");
        assert_eq!(calls.get(), [1, 1, 1]);
        // A week later, nothing changed: the terrain source is checked again, its
        // outputs are identical, the derived steps are taken over.
        let mut old = o1.steps.clone();
        for s in &mut old {
            s.at = Some("2000-01-01T00:00:00Z".into());
        }
        let o2 = go(Some(resume(&old)), b"dem");
        assert_eq!(
            calls.get(),
            [2, 1, 1],
            "only the external source is redone by age: {:?}",
            o2.steps
        );
        assert!(o2.steps[1].resumed && o2.steps[2].resumed);
        // The source changed: what reads the terrain follows.
        let mut old = o2.steps.clone();
        old[0].at = Some("2000-01-01T00:00:00Z".into());
        go(Some(resume(&old)), b"dem2");
        assert_eq!(calls.get(), [3, 2, 2]);
    }

    #[test]
    fn a_panic_fails_its_step_only() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = Cell::new([0; 3]);
        let journal = Mutex::new(Vec::new());
        let o = run(tmp.path(), None, "panic", &calls, &journal);
        assert_eq!(calls.get(), [1, 1, 1]);
        assert_eq!(o.steps[1].state, StepState::Failed);
        assert!(
            o.steps[1].line.contains("panique : boom"),
            "{}",
            o.steps[1].line
        );
        assert_eq!(o.steps[2].state, StepState::Ok);
    }

    /// Every step of the pipeline, as stand-ins: each writes in its output directory what
    /// it saw of the site (its outputs depend on what it read). Returns the outcome and
    /// the steps run (not taken over).
    fn every_step(
        dir: &Path,
        site: &Site,
        resume: Option<Resume<'_>>,
    ) -> (BuildOutcome, Vec<&'static str>) {
        let shared = Shared {
            resume,
            ..Shared::default()
        };
        let ran = std::cell::RefCell::new(Vec::new());
        let mut log = Vec::new();
        let mut r = Run::new(site, dir.to_path_buf(), &shared, &mut log);
        for (name, fields) in READS {
            r.step(name, &[], |v| {
                ran.borrow_mut().push(*name);
                // The view holds the declared fields only.
                assert_eq!(
                    v.starts.is_empty(),
                    !fields.contains(&Field::Starts) || site.starts.is_empty(),
                    "{name}"
                );
                assert_eq!(
                    v.status.is_empty(),
                    !fields.contains(&Field::Status) || site.status.is_empty(),
                    "{name}"
                );
                if let Some(d) = step_dir(name) {
                    std::fs::create_dir_all(dir.join(d))?;
                    std::fs::write(dir.join(d).join("out.json"), serde_json::to_vec(v)?)?;
                }
                Ok((*name).to_owned())
            });
        }
        let o = r.finish();
        (o, ran.into_inner())
    }

    fn with_starts(site: &Site, n: usize) -> Site {
        let starts: Vec<skimap_common::site::Start> = (0..n)
            .map(|i| skimap_common::site::Start {
                id: format!("d{i}"),
                name: format!("Départ {i}"),
                phrase: format!("le départ {i}"),
                phrase_en: None,
                lon: site.commune_center.lon + i as f64 * 0.001,
                lat: site.commune_center.lat,
                provenance: "test".into(),
                access_m: None,
            })
            .collect();
        Site {
            starts: Box::leak(starts.into_boxed_slice()),
            ..*site
        }
    }

    /// #164: a change of the starts of a site (X9, after new station cards) runs again
    /// only the steps that read them (and those taken over by none: no output
    /// directory); a change of its working area runs every step again.
    #[test]
    fn a_change_of_the_starts_runs_again_only_the_steps_that_read_them() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let one = with_starts(site, 1);
        let (o1, ran) = every_step(dir, &one, None);
        assert!(o1.ok());
        assert_eq!(ran.len(), READS.len());
        // Same record: everything with an output directory is taken over.
        let (o2, ran) = every_step(dir, &one, Some(resume(&o1.steps)));
        let never_taken_over: Vec<&str> = READS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| step_dir(n).is_none())
            .collect();
        assert_eq!(ran, never_taken_over);
        // One more start.
        let two = with_starts(site, 2);
        let (o3, ran) = every_step(dir, &two, Some(resume(&o2.steps)));
        let expected: Vec<&str> = READS
            .iter()
            .filter(|(n, f)| f.contains(&Field::Starts) || step_dir(n).is_none())
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(ran, expected);
        assert_eq!(
            ran.iter()
                .filter(|n| step_dir(n).is_some())
                .copied()
                .collect::<Vec<_>>(),
            ["meteo", "build-pack", "export"]
        );
        for s in &o3.steps {
            assert_eq!(
                s.inputs.as_deref(),
                Some(step_inputs(&s.name, &two).as_str())
            );
        }
        // Another working area.
        let mut moved = two;
        moved.bbox.north += 0.01;
        let (_, ran) = every_step(dir, &moved, Some(resume(&o3.steps)));
        assert_eq!(ran.len(), READS.len(), "{ran:?}");
    }

    /// A record of a step made before #164 has no fingerprint of what it read: not taken
    /// over (the batch fills it from the site record of that build, `with_step_inputs`).
    #[test]
    fn a_step_without_inputs_fingerprint_is_run_again() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let (o1, _) = every_step(dir, site, None);
        let mut old = o1.steps.clone();
        for s in &mut old {
            s.inputs = None;
        }
        let (_, ran) = every_step(dir, site, Some(resume(&old)));
        assert_eq!(ran.len(), READS.len());
    }

    /// Every step of [`Ctx::build_all_with`] declares the fields of the site record it
    /// reads, and none can read another one: the steps get the site only as the view of
    /// their declared fields (closure argument; `Ctx::steps` never names `run.site`, and
    /// the `Run` is borrowed by the step while it runs).
    #[test]
    fn every_step_declares_what_it_reads_and_sees_nothing_else() {
        let src = include_str!("build.rs");
        let start = src
            .find("    fn steps(&self, run: &mut Run<'_>")
            .expect("Ctx::steps");
        let body = &src[start..];
        let body = &body[..body.find("\n    }\n").expect("end of Ctx::steps")];
        assert!(!body.contains("run.site"), "a step reads the whole site");
        let mut names = Vec::new();
        for (i, _) in body.match_indices("run.step(") {
            let rest = body[i + "run.step(".len()..].trim_start();
            let name = rest[1..].split('"').next().unwrap();
            names.push(name.to_owned());
        }
        assert!(names.len() >= 20, "{names:?}");
        for n in &names {
            assert!(
                READS.iter().any(|(r, _)| r == n),
                "step {n} does not declare the fields it reads (READS)"
            );
        }
        for (r, _) in READS {
            assert!(names.iter().any(|n| n == r), "READS lists {r}, not a step");
        }
        // The view of a step holds its fields only.
        let site = with_starts(skimap_common::site::site("cervieres").unwrap(), 1);
        let terrain = view(&site, reads("terrain"));
        assert!(terrain.starts.is_empty() && terrain.status.is_empty());
        assert_eq!(terrain.bbox, site.bbox);
        assert_eq!(view(&site, reads("meteo")).starts.len(), 1);
        assert_eq!(view(&site, reads("planet")).bbox.north, 0.0);
    }

    /// Chaos: a run killed after k steps (journal of the k first steps, outputs of a
    /// half-done step left as a temporary file), then resumed, gives the outputs of a run
    /// without incident; at least 20 interruption points.
    #[test]
    fn interrupted_runs_resume_to_the_same_result() {
        let reference = {
            let tmp = tempfile::tempdir().unwrap();
            let o = run(
                tmp.path(),
                None,
                "v1",
                &Cell::new([0; 3]),
                &Mutex::new(Vec::new()),
            );
            assert!(o.ok());
            (
                std::fs::read(tmp.path().join("pack/p")).unwrap(),
                fingerprint(tmp.path()),
            )
        };
        let mut trials = 0;
        for kill_after in 0..=3usize {
            for garbage in 0..6u8 {
                trials += 1;
                let tmp = tempfile::tempdir().unwrap();
                let dir = tmp.path();
                let journal = Mutex::new(Vec::new());
                // Full run, then « kill »: keep only the journal of the first steps, and
                // drop or damage the outputs of the steps after them.
                let full = run(dir, None, "v1", &Cell::new([0; 3]), &journal);
                let names = ["terrain", "refuges", "build-pack"];
                for (i, n) in names.iter().enumerate().skip(kill_after) {
                    let d = dir.join(step_dir(n).unwrap());
                    match (garbage + i as u8) % 3 {
                        0 => {
                            let _ = std::fs::remove_dir_all(&d);
                        }
                        1 => {
                            let f = std::fs::read_dir(&d).unwrap().next().unwrap().unwrap();
                            std::fs::write(f.path(), &[garbage][..]).unwrap();
                        }
                        _ => {
                            std::fs::write(d.join("x.tmp"), b"half").unwrap();
                            let f = std::fs::read_dir(&d)
                                .unwrap()
                                .flatten()
                                .find(|e| !e.file_name().to_string_lossy().contains(".tmp"))
                                .unwrap();
                            std::fs::write(f.path(), b"").unwrap();
                        }
                    }
                }
                let journaled: Vec<StepRecord> = full.steps[..kill_after].to_vec();
                crate::batch::remove_orphan_tmp(dir);
                let calls = Cell::new([0; 3]);
                let o = run(dir, Some(resume(&journaled)), "v1", &calls, &journal);
                assert!(o.ok());
                for (i, s) in o.steps.iter().enumerate() {
                    assert_eq!(s.resumed, i < kill_after, "trial {trials}: {s:?}");
                }
                assert_eq!(
                    std::fs::read(dir.join("pack/p")).unwrap(),
                    reference.0,
                    "trial {trials}"
                );
                assert_eq!(fingerprint(dir), reference.1, "trial {trials}");
            }
        }
        assert!(trials >= 20);
    }
}

/// A source down, then back: the first run keeps the previous outputs of the failed step
/// (with their date) and still builds the pack; the next run sends requests only for the
/// failed step. Local HTTP server, real downloader, requests counted.
#[cfg(test)]
mod outage_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    struct Server {
        base: String,
        hits: Arc<Mutex<BTreeMap<String, usize>>>,
        down: Arc<AtomicBool>,
        server: Arc<tiny_http::Server>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Server {
        /// `/terrain` always answers; `/refuges` answers `v<n>` (n-th good answer), or
        /// 502 while `down`.
        fn start() -> Self {
            let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let port = server.server_addr().to_ip().unwrap().port();
            let hits = Arc::new(Mutex::new(BTreeMap::new()));
            let down = Arc::new(AtomicBool::new(false));
            let (srv, h, d) = (server.clone(), hits.clone(), down.clone());
            let thread = std::thread::spawn(move || {
                let mut good = 0;
                for rq in srv.incoming_requests() {
                    let path = rq.url().to_owned();
                    *h.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
                    let resp = if path == "/refuges" && d.load(Ordering::SeqCst) {
                        tiny_http::Response::from_string("bad gateway").with_status_code(502)
                    } else if path == "/refuges" {
                        good += 1;
                        tiny_http::Response::from_string(format!("v{good}"))
                    } else {
                        tiny_http::Response::from_string("dem")
                    };
                    let _ = rq.respond(resp);
                }
            });
            Self {
                base: format!("http://127.0.0.1:{port}"),
                hits,
                down,
                server,
                thread: Some(thread),
            }
        }

        fn hits(&self) -> BTreeMap<String, usize> {
            self.hits.lock().unwrap().clone()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.server.unblock();
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn run(dir: &Path, dl: &Downloader, base: &str, resume: Option<Resume<'_>>) -> BuildOutcome {
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let shared = Shared {
            resume,
            ..Shared::default()
        };
        let mut log = Vec::new();
        let mut r = Run::new(site, dir.to_path_buf(), &shared, &mut log);
        let get = |path: &str| -> Result<Vec<u8>> {
            // Max age 0: every run asks the source (the cache would hide the outage).
            let req = skimap_common::download::Request::get("t", &format!("{base}{path}"))
                .max_age(std::time::Duration::ZERO);
            Ok(dl.fetch(&req)?.bytes()?)
        };
        r.step("terrain", &[], |_| {
            let b = get("/terrain")?;
            std::fs::create_dir_all(dir.join("terrain"))?;
            std::fs::write(dir.join("terrain/dem"), b)?;
            Ok("terrain".into())
        });
        r.step("refuges", &[], |_| {
            let b = get("/refuges")?;
            std::fs::create_dir_all(dir.join("refuges"))?;
            std::fs::write(dir.join("refuges/r"), b)?;
            Ok("refuges".into())
        });
        r.step("build-pack", &[], |_| {
            std::fs::create_dir_all(dir.join("pack"))?;
            let t = std::fs::read(dir.join("terrain/dem"))?;
            let f = std::fs::read(dir.join("refuges/r")).unwrap_or_default();
            std::fs::write(dir.join("pack/p"), [t, f].concat())?;
            Ok("pack".into())
        });
        r.finish()
    }

    fn resume(steps: &[StepRecord], max_age_s: u64) -> Resume<'_> {
        Resume {
            previous: steps,
            max_age: std::time::Duration::from_secs(max_age_s),
            pack_bytes: None,
            export_bytes: None,
            personal_sources: false,
        }
    }

    #[test]
    fn a_source_down_keeps_the_previous_outputs_and_only_the_failed_step_runs_again() {
        let srv = Server::start();
        let tmp = tempfile::tempdir().unwrap();
        let (dir, cache) = (tmp.path().join("out"), tmp.path().join("cache"));
        let dl = Downloader::builder(&cache)
            .policy(
                skimap_common::download::SourcePolicy::new("t")
                    .limits(Vec::new())
                    .retries(
                        2,
                        std::time::Duration::from_millis(1),
                        std::time::Duration::from_millis(5),
                    ),
            )
            .build()
            .unwrap();
        // A first good build.
        let o0 = run(&dir, &dl, &srv.base, None);
        assert!(o0.ok());
        assert_eq!(std::fs::read(dir.join("pack/p")).unwrap(), b"demv1");
        // The refuges source goes down; everything is due again (max age 0).
        srv.down.store(true, Ordering::SeqCst);
        let o1 = run(&dir, &dl, &srv.base, Some(resume(&o0.steps, 0)));
        assert_eq!(o1.steps[1].state, StepState::Failed);
        assert!(o1.steps[1].line.contains("502"), "{}", o1.steps[1].line);
        // The previous outputs stay, the pack is built with them.
        assert_eq!(std::fs::read(dir.join("refuges/r")).unwrap(), b"v1");
        assert_eq!(o1.steps[2].state, StepState::Ok);
        assert_eq!(std::fs::read(dir.join("pack/p")).unwrap(), b"demv1");
        let h1 = srv.hits();
        assert_eq!(h1["/terrain"], 2);
        assert_eq!(h1["/refuges"], 1 + 3, "one good answer, then 1 + 2 retries");
        // The source is back: the next run (normal max age) asks only for the failed
        // step; terrain is taken over without a request.
        srv.down.store(false, Ordering::SeqCst);
        let o2 = run(&dir, &dl, &srv.base, Some(resume(&o1.steps, 3600)));
        assert!(o2.ok(), "{:?}", o2.steps);
        assert!(o2.steps[0].resumed && !o2.steps[1].resumed);
        let h2 = srv.hits();
        assert_eq!(h2["/terrain"], h1["/terrain"], "no request for terrain");
        assert_eq!(h2["/refuges"], h1["/refuges"] + 1);
        assert_eq!(std::fs::read(dir.join("pack/p")).unwrap(), b"demv2");
        // Nothing left to catch up: no request at all.
        let o3 = run(&dir, &dl, &srv.base, Some(resume(&o2.steps, 3600)));
        assert!(o3.ok() && o3.steps.iter().all(|s| s.resumed));
        assert_eq!(srv.hits(), h2);
    }
}
