//! Météo-France hourly climatological data (BDClim, "Données climatologiques de base -
//! horaires", data.gouv, Licence Ouverte): snow depth `NEIGETOT` of the validation
//! stations.
//!
//! `NEIGETOT` is the "hauteur de neige totale au sol" in **cm** (integer), per the official
//! `H_descriptif_champs.csv` and the PDF description of 2026-06-03. Hours (`AAAAMMJJHH`)
//! are UTC in metropolitan France (data.gouv dataset description). Quality code `QNEIGETOT`: 9 filtered, 0 protected, 1 validated, 2 doubtful
//! (being checked); doubtful values are dropped. Files are per department and period:
//! `H_<dep>_previous-2020-2024.csv.gz` and `H_<dep>_latest-2025-2026.csv.gz`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use skimap_common::download::{Downloader, Request};

use crate::error::{Error, Result};
use crate::sim::SOURCE;

/// A validation station.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StationDef {
    /// Météo-France station number (`NUM_POSTE`).
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// Site it validates.
    pub site: &'static str,
    /// Department of the hourly file.
    pub department: &'static str,
}

/// Stations named in P7 (#20): Cervières and Arvieux (Queyras); Mouthe and La Boissaude
/// (Rochejean) in the Haut-Doubs, a few km outside the Chapelle-des-Bois box.
pub const STATIONS: [StationDef; 4] = [
    StationDef {
        id: "05027001",
        name: "CERVIERES",
        site: "cervieres",
        department: "05",
    },
    StationDef {
        id: "05007003",
        name: "ARVIEUX",
        site: "cervieres",
        department: "05",
    },
    StationDef {
        id: "25413001",
        name: "MOUTHE",
        site: "chapelle-des-bois",
        department: "25",
    },
    StationDef {
        id: "25494001",
        name: "LA BOISSAUDE ROCHEJEAN",
        site: "chapelle-des-bois",
        department: "25",
    },
];

/// Hour (UTC) of the observation compared with the simulation (state at 06 UTC).
pub const OBS_HOUR: u32 = 6;

/// Hourly file URLs of a department covering 2020 to 2026.
pub fn department_urls(dep: &str) -> [String; 2] {
    let base = "https://meteofrance.s3.sbg.io.cloud.ovh.net/data/synchro_ftp/BASE/HOR";
    [
        format!("{base}/H_{dep}_previous-2020-2024.csv.gz"),
        format!("{base}/H_{dep}_latest-2025-2026.csv.gz"),
    ]
}

/// Description of the hourly fields (unit of `NEIGETOT`).
pub const FIELDS_URL: &str =
    "https://meteofrance.s3.sbg.io.cloud.ovh.net/data/synchro_ftp/BASE/HOR/H_descriptif_champs.csv";

/// Metadata and daily 06 UTC snow depths of one station.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StationSeries {
    /// Station number.
    pub id: String,
    /// Name (`NOM_USUEL`).
    pub name: String,
    /// Latitude, degrees.
    pub lat: f64,
    /// Longitude, degrees.
    pub lon: f64,
    /// Altitude, m.
    pub alti: f64,
    /// Snow depth (cm) at 06 UTC by date.
    pub depth_cm: BTreeMap<NaiveDate, f32>,
    /// Values dropped because flagged doubtful.
    pub doubtful_dropped: usize,
}

/// Parses an hourly CSV (gzipped) and keeps the 06 UTC `NEIGETOT` of `ids`.
pub fn parse_gz(gz: &[u8], ids: &[&str], into: &mut BTreeMap<String, StationSeries>) -> Result<()> {
    let reader = BufReader::with_capacity(1 << 20, flate2::read::GzDecoder::new(gz));
    parse_reader(reader, ids, into)
}

/// Parses hourly CSV text (see [`parse_gz`]).
pub fn parse_reader(
    reader: impl BufRead,
    ids: &[&str],
    into: &mut BTreeMap<String, StationSeries>,
) -> Result<()> {
    let err = |m: String| Error::parse("hourly CSV", m);
    let mut lines = reader.lines();
    let header = lines
        .next()
        .ok_or_else(|| err("empty file".into()))?
        .map_err(|e| err(e.to_string()))?;
    let cols: Vec<&str> = header.trim_end().split(';').collect();
    let idx = |name: &str| {
        cols.iter()
            .position(|c| *c == name)
            .ok_or_else(|| err(format!("missing column {name}")))
    };
    if idx("NUM_POSTE")? != 0 {
        return Err(err("NUM_POSTE is not the first column".into()));
    }
    let (i_name, i_lat, i_lon, i_alt, i_date, i_snow, i_q) = (
        idx("NOM_USUEL")?,
        idx("LAT")?,
        idx("LON")?,
        idx("ALTI")?,
        idx("AAAAMMJJHH")?,
        idx("NEIGETOT")?,
        idx("QNEIGETOT")?,
    );
    let hour = format!("{OBS_HOUR:02}");
    for line in lines {
        let line = line.map_err(|e| err(e.to_string()))?;
        let Some(id) = line.split(';').next() else {
            continue;
        };
        if !ids.contains(&id) {
            continue;
        }
        let f: Vec<&str> = line.split(';').collect();
        let get = |i: usize| f.get(i).copied().unwrap_or("");
        let date = get(i_date);
        if date.len() != 10 || &date[8..10] != hour.as_str() {
            continue;
        }
        let s = into.entry(id.to_owned()).or_default();
        if s.id.is_empty() {
            s.id = id.to_owned();
            s.name = get(i_name).to_owned();
            s.lat = get(i_lat).parse().map_err(|e| err(format!("LAT: {e}")))?;
            s.lon = get(i_lon).parse().map_err(|e| err(format!("LON: {e}")))?;
            s.alti = get(i_alt).parse().map_err(|e| err(format!("ALTI: {e}")))?;
        }
        let v = get(i_snow);
        if v.is_empty() {
            continue;
        }
        if get(i_q) == "2" {
            s.doubtful_dropped += 1;
            continue;
        }
        let depth: f32 = v.parse().map_err(|e| err(format!("NEIGETOT {v:?}: {e}")))?;
        let d: u32 = date[..8]
            .parse()
            .map_err(|e| err(format!("date {date}: {e}")))?;
        let day = NaiveDate::from_ymd_opt((d / 10_000) as i32, d / 100 % 100, d % 100)
            .ok_or_else(|| err(format!("bad date {date}")))?;
        s.depth_cm.insert(day, depth);
    }
    Ok(())
}

/// Downloads (or reads from cache) and parses the hourly files of the stations of `defs`.
pub fn fetch(dl: &Downloader, defs: &[StationDef]) -> Result<BTreeMap<String, StationSeries>> {
    let mut out = BTreeMap::new();
    let mut deps: Vec<&str> = defs.iter().map(|d| d.department).collect();
    deps.sort_unstable();
    deps.dedup();
    for dep in deps {
        let ids: Vec<&str> = defs
            .iter()
            .filter(|d| d.department == dep)
            .map(|d| d.id)
            .collect();
        for url in department_urls(dep) {
            let bytes = dl.fetch(&Request::get(SOURCE, &url))?.bytes()?;
            parse_gz(&bytes, &ids, &mut out)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_06_utc_and_drops_doubtful() {
        let csv = "NUM_POSTE;NOM_USUEL;LAT;LON;ALTI;AAAAMMJJHH;RR1;NEIGETOT;QNEIGETOT\n\
                   05027001;CERVIERES;44.869333;6.723500;1614;2025010105;;31;9\n\
                   05027001;CERVIERES;44.869333;6.723500;1614;2025010106;;32;9\n\
                   05027001;CERVIERES;44.869333;6.723500;1614;2025010206;;99;2\n\
                   05027001;CERVIERES;44.869333;6.723500;1614;2025010306;;;\n\
                   05001400;ABRIES;44.79;6.94;2018;2025010106;;41;1\n";
        let mut out = BTreeMap::new();
        parse_reader(csv.as_bytes(), &["05027001"], &mut out).unwrap();
        let s = &out["05027001"];
        assert_eq!(s.name, "CERVIERES");
        assert_eq!(s.alti, 1614.0);
        assert_eq!(s.depth_cm.len(), 1);
        assert_eq!(
            s.depth_cm[&NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()],
            32.0
        );
        assert_eq!(s.doubtful_dropped, 1);
        assert!(!out.contains_key("05001400"));
    }
}
