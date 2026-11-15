//! Minimal single-band GeoTIFF reader and writer: Float32 read and write, UInt8 read
//! (class rasters), and a lenient reader of any common sample type and CRS
//! ([`read_any`]).
//!
//! Writes north-up grids with the GeoTIFF tags GDAL and QGIS read: `ModelPixelScale`,
//! `ModelTiepoint`, `GeoKeyDirectory` (EPSG code, pixel-is-area) and `GDAL_NODATA`.
//! In memory, no-data is always `NaN`; on disk it is written as `nan`.
//!
//! Reads the same subset, which is what the IGN Géoplateforme WMS returns for
//! `FORMAT=image/geotiff` (uncompressed Float32, `GDAL_NODATA = -9999`).
//!
//! Feature `geotiff` (pipeline crates writing or reading rasters).

use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::path::Path;

use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::encoder::{Compression, DeflateLevel, TiffEncoder, colortype::Gray32Float};
use tiff::tags::Tag;

use crate::crs::Crs;
use crate::error::{Error, Result};
use crate::grid::{GeoTransform, Grid};

// GeoTIFF key identifiers (GeoTIFF 1.1, section 7).
const GT_MODEL_TYPE: u16 = 1024;
const GT_RASTER_TYPE: u16 = 1025;
const GEOGRAPHIC_TYPE: u16 = 2048;
const GEOG_ANGULAR_UNITS: u16 = 2054;
const PROJECTED_CS_TYPE: u16 = 3072;
const PROJ_LINEAR_UNITS: u16 = 3076;
const MODEL_TYPE_PROJECTED: u16 = 1;
const MODEL_TYPE_GEOGRAPHIC: u16 = 2;
const RASTER_PIXEL_IS_AREA: u16 = 1;
const LINEAR_METER: u16 = 9001;
const ANGULAR_DEGREE: u16 = 9102;

/// GeoKeyDirectory of a CRS.
fn geo_keys(crs: Crs) -> Vec<u16> {
    let epsg = u16::try_from(crs.epsg()).expect("supported EPSG codes fit in u16");
    let keys: Vec<[u16; 4]> = if crs.is_geographic() {
        vec![
            [GT_MODEL_TYPE, 0, 1, MODEL_TYPE_GEOGRAPHIC],
            [GT_RASTER_TYPE, 0, 1, RASTER_PIXEL_IS_AREA],
            [GEOGRAPHIC_TYPE, 0, 1, epsg],
            [GEOG_ANGULAR_UNITS, 0, 1, ANGULAR_DEGREE],
        ]
    } else {
        vec![
            [GT_MODEL_TYPE, 0, 1, MODEL_TYPE_PROJECTED],
            [GT_RASTER_TYPE, 0, 1, RASTER_PIXEL_IS_AREA],
            [PROJECTED_CS_TYPE, 0, 1, epsg],
            [PROJ_LINEAR_UNITS, 0, 1, LINEAR_METER],
        ]
    };
    let n = u16::try_from(keys.len()).expect("few keys");
    let mut out = vec![1, 1, 0, n];
    out.extend(keys.into_iter().flatten());
    out
}

/// Value of a GeoKey: inline short, doubles (tag 34736) or ASCII (tag 34737).
#[derive(Debug, Clone, PartialEq)]
enum KeyValue {
    Short(u16),
    Doubles(Vec<f64>),
    Ascii(String),
}

fn parse_keys(keys: &[u16], doubles: &[f64], ascii: &str) -> Vec<(u16, KeyValue)> {
    let mut out = Vec::new();
    for k in keys.get(4..).unwrap_or_default().as_chunks::<4>().0 {
        let (id, loc, count, off) = (k[0], k[1], usize::from(k[2]), usize::from(k[3]));
        let v = match loc {
            0 => KeyValue::Short(k[3]),
            34736 => KeyValue::Doubles(doubles.get(off..off + count).unwrap_or_default().to_vec()),
            34737 => KeyValue::Ascii(ascii.get(off..off + count).unwrap_or_default().to_owned()),
            _ => continue,
        };
        out.push((id, v));
    }
    out
}

const USER_DEFINED: u16 = 32767;
const PROJ_COORD_TRANS: u16 = 3075;
const CT_LAMBERT_CONF_CONIC_2SP: u16 = 8;
/// Lambert-93 parameters as GeoKeys: StdParallel1, StdParallel2, FalseOriginLong,
/// FalseOriginLat, FalseOriginEasting, FalseOriginNorthing.
const LAMBERT93_PARAMS: [(u16, f64); 6] = [
    (3078, 49.0),
    (3079, 44.0),
    (3084, 3.0),
    (3085, 46.5),
    (3086, 700_000.0),
    (3087, 6_600_000.0),
];
const GT_CITATION: u16 = 1026;
const PCS_CITATION: u16 = 3073;

/// CRS of a GeoKey directory. Accepts EPSG codes, and the user-defined Lambert Conformal
/// Conic with the Lambert-93 parameters that the Géoplateforme WMS writes (with the
/// citation `EPSG:2154`).
fn crs_from_keys(keys: &[u16], doubles: &[f64], ascii: &str) -> Result<Crs> {
    let kv = parse_keys(keys, doubles, ascii);
    let short = |id| {
        kv.iter().find_map(|(k, v)| match v {
            KeyValue::Short(s) if *k == id => Some(*s),
            _ => None,
        })
    };
    let double = |id| {
        kv.iter().find_map(|(k, v)| match v {
            KeyValue::Doubles(d) if *k == id && d.len() == 1 => Some(d[0]),
            _ => None,
        })
    };
    let code = short(PROJECTED_CS_TYPE).or(short(GEOGRAPHIC_TYPE));
    match code {
        Some(2154) => return Ok(Crs::Lambert93),
        Some(3857) => return Ok(Crs::WebMercator),
        Some(4326) if short(PROJECTED_CS_TYPE).is_none() => return Ok(Crs::Wgs84),
        _ => {}
    }
    if short(PROJECTED_CS_TYPE) == Some(USER_DEFINED)
        && short(PROJ_COORD_TRANS) == Some(CT_LAMBERT_CONF_CONIC_2SP)
        && LAMBERT93_PARAMS
            .iter()
            .all(|(id, v)| double(*id).is_some_and(|d| (d - v).abs() < 1e-9))
    {
        let cites_2154 = kv.iter().any(|(k, v)| {
            matches!(v, KeyValue::Ascii(s) if (*k == GT_CITATION || *k == PCS_CITATION) && s.contains("2154"))
        });
        if cites_2154 {
            return Ok(Crs::Lambert93);
        }
    }
    Err(Error::tiff(
        "geokeys",
        format!(
            "unsupported CRS: projected {:?}, geographic {:?}",
            short(PROJECTED_CS_TYPE),
            short(GEOGRAPHIC_TYPE)
        ),
    ))
}

/// Encodes a Float32 grid as a Deflate-compressed GeoTIFF into `w`.
pub fn encode_f32<W: Write + Seek>(w: W, grid: &Grid<f32>) -> Result<()> {
    let ctx = "write";
    let width = u32::try_from(grid.width()).map_err(|e| Error::tiff(ctx, e))?;
    let height = u32::try_from(grid.height()).map_err(|e| Error::tiff(ctx, e))?;
    let mut enc = TiffEncoder::new(w)
        .map_err(|e| Error::tiff(ctx, e))?
        .with_compression(Compression::Deflate(DeflateLevel::Best));
    let mut img = enc
        .new_image::<Gray32Float>(width, height)
        .map_err(|e| Error::tiff(ctx, e))?;
    let gt = grid.transform();
    let d = img.encoder();
    d.write_tag(
        Tag::ModelPixelScaleTag,
        &[gt.pixel_width, -gt.pixel_height, 0.0][..],
    )
    .map_err(|e| Error::tiff(ctx, e))?;
    d.write_tag(
        Tag::ModelTiepointTag,
        &[0.0, 0.0, 0.0, gt.origin_x, gt.origin_y, 0.0][..],
    )
    .map_err(|e| Error::tiff(ctx, e))?;
    d.write_tag(Tag::GeoKeyDirectoryTag, &geo_keys(grid.crs())[..])
        .map_err(|e| Error::tiff(ctx, e))?;
    d.write_tag(Tag::GdalNodata, "nan")
        .map_err(|e| Error::tiff(ctx, e))?;
    img.write_data(grid.data())
        .map_err(|e| Error::tiff(ctx, e))?;
    Ok(())
}

/// Writes a Float32 grid as a GeoTIFF file atomically (`<file>.tmp` then rename);
/// returns the file size in bytes.
pub fn write_f32(path: &Path, grid: &Grid<f32>) -> Result<u64> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let f = File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    let mut w = BufWriter::new(f);
    encode_f32(&mut w, grid)?;
    w.flush().map_err(|e| Error::io(&tmp, e))?;
    drop(w);
    let len = std::fs::metadata(&tmp)
        .map_err(|e| Error::io(&tmp, e))?
        .len();
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))?;
    Ok(len)
}

/// Georeferencing and no-data of a decoded file.
struct Header {
    width: usize,
    height: usize,
    transform: GeoTransform,
    /// CRS when supported ([`crs_from_keys`]), else the error it gave.
    crs: Result<Crs>,
    /// EPSG code of the projected (else geographic) CRS when the keys state one.
    epsg: Option<u16>,
    /// `GDAL_NODATA` as written.
    nodata: Option<String>,
}

fn decoder(bytes: &[u8]) -> Result<Decoder<Cursor<&[u8]>>> {
    Ok(Decoder::new(Cursor::new(bytes))
        .map_err(|e| Error::tiff("read", e))?
        .with_limits(Limits::unlimited()))
}

fn header(dec: &mut Decoder<Cursor<&[u8]>>) -> Result<Header> {
    let ctx = "read";
    let (w, h) = dec.dimensions().map_err(|e| Error::tiff(ctx, e))?;
    let scale = dec
        .get_tag_f64_vec(Tag::ModelPixelScaleTag)
        .map_err(|e| Error::tiff("ModelPixelScale", e))?;
    let tie = dec
        .get_tag_f64_vec(Tag::ModelTiepointTag)
        .map_err(|e| Error::tiff("ModelTiepoint", e))?;
    let keys = dec
        .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
        .map_err(|e| Error::tiff("GeoKeyDirectory", e))?;
    let nodata = match dec.find_tag(Tag::GdalNodata) {
        Ok(Some(v)) => {
            let s = v.into_string().map_err(|e| Error::tiff("GDAL_NODATA", e))?;
            Some(
                s.trim_matches(|c: char| c == '\0' || c.is_whitespace())
                    .to_owned(),
            )
        }
        Ok(None) => None,
        Err(e) => return Err(Error::tiff("GDAL_NODATA", e)),
    };
    if scale.len() < 2 || tie.len() < 6 {
        return Err(Error::tiff(ctx, "incomplete georeferencing tags"));
    }
    if (scale[0] - scale[1]).abs() > 1e-9 {
        return Err(Error::tiff(ctx, format!("non-square cells {scale:?}")));
    }
    // Tie point (i, j) -> (x, y); origin of the raster is pixel (0, 0).
    let origin_x = tie[3] - tie[0] * scale[0];
    let origin_y = tie[4] + tie[1] * scale[1];
    let doubles = match dec.find_tag(Tag::GeoDoubleParamsTag) {
        Ok(Some(v)) => v
            .into_f64_vec()
            .map_err(|e| Error::tiff("GeoDoubleParams", e))?,
        Ok(None) => Vec::new(),
        Err(e) => return Err(Error::tiff("GeoDoubleParams", e)),
    };
    let ascii = match dec.find_tag(Tag::GeoAsciiParamsTag) {
        Ok(Some(v)) => v
            .into_string()
            .map_err(|e| Error::tiff("GeoAsciiParams", e))?,
        Ok(None) => String::new(),
        Err(e) => return Err(Error::tiff("GeoAsciiParams", e)),
    };
    let kv = parse_keys(&keys, &doubles, &ascii);
    let short = |id| {
        kv.iter().find_map(|(k, v)| match v {
            KeyValue::Short(s) if *k == id => Some(*s),
            _ => None,
        })
    };
    let epsg = short(PROJECTED_CS_TYPE)
        .or(short(GEOGRAPHIC_TYPE))
        .filter(|c| *c != USER_DEFINED);
    Ok(Header {
        width: w as usize,
        height: h as usize,
        transform: GeoTransform::north_up(origin_x, origin_y, scale[0]),
        crs: crs_from_keys(&keys, &doubles, &ascii),
        epsg,
        nodata,
    })
}

/// Decodes a single-band Float32 GeoTIFF; no-data values become `NaN`.
pub fn decode_f32(bytes: &[u8]) -> Result<Grid<f32>> {
    let ctx = "read";
    let mut dec = decoder(bytes)?;
    let h = header(&mut dec)?;
    let crs = h.crs?;
    let nodata: Option<f32> = h
        .nodata
        .as_deref()
        .map(|s| {
            s.parse::<f32>()
                .map_err(|e| Error::tiff("GDAL_NODATA", format!("{s:?}: {e}")))
        })
        .transpose()?;
    let mut data = match dec.read_image().map_err(|e| Error::tiff(ctx, e))? {
        DecodingResult::F32(v) => v,
        _ => return Err(Error::tiff(ctx, "not a Float32 raster")),
    };
    if let Some(nd) = nodata
        && !nd.is_nan()
    {
        for v in &mut data {
            if *v == nd {
                *v = f32::NAN;
            }
        }
    }
    Grid::from_vec(h.width, h.height, h.transform, crs, data)
}

/// Reads a single-band Float32 GeoTIFF file.
pub fn read_f32(path: &Path) -> Result<Grid<f32>> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    decode_f32(&bytes)
}

/// Decodes a single-band UInt8 GeoTIFF (class rasters: avalanche exposure); the grid
/// carries the `GDAL_NODATA` value when the file states one in 0..=255.
pub fn decode_u8(bytes: &[u8]) -> Result<Grid<u8>> {
    let ctx = "read";
    let mut dec = decoder(bytes)?;
    let h = header(&mut dec)?;
    let crs = h.crs?;
    let nodata = h
        .nodata
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.fract() == 0.0 && (0.0..=255.0).contains(v))
        .map(|v| v as u8);
    let data = match dec.read_image().map_err(|e| Error::tiff(ctx, e))? {
        DecodingResult::U8(v) => v,
        _ => return Err(Error::tiff(ctx, "not a UInt8 raster")),
    };
    let g = Grid::from_vec(h.width, h.height, h.transform, crs, data)?;
    Ok(match nodata {
        Some(nd) => g.with_nodata(nd),
        None => g,
    })
}

/// Reads a single-band UInt8 GeoTIFF file.
pub fn read_u8(path: &Path) -> Result<Grid<u8>> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    decode_u8(&bytes)
}

/// A GeoTIFF of any common sample type read by [`decode_any`]: values as `f32`, no-data
/// and non-finite values as `NaN`; the CRS may be one this crate does not handle.
#[derive(Debug, Clone, PartialEq)]
pub struct AnyRaster {
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
    /// North-up georeferencing.
    pub transform: GeoTransform,
    /// CRS when supported (Lambert-93, WGS84, Web Mercator).
    pub crs: Option<Crs>,
    /// EPSG code stated by the file, supported or not.
    pub epsg: Option<u16>,
    /// Values, row-major.
    pub data: Vec<f32>,
}

/// Decodes a single-band GeoTIFF of any common sample type (UInt8, UInt16, Int16, Int32,
/// Float32, Float64), striped or tiled, uncompressed, LZW or Deflate: rasters of other
/// tools (GDAL, the AutoATES reference run), in any CRS.
pub fn decode_any(bytes: &[u8]) -> Result<AnyRaster> {
    let ctx = "read";
    let mut dec = decoder(bytes)?;
    let h = header(&mut dec)?;
    let data: Vec<f32> = match dec.read_image().map_err(|e| Error::tiff(ctx, e))? {
        DecodingResult::U8(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::U16(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::I16(v) => v.into_iter().map(f32::from).collect(),
        DecodingResult::I32(v) => v.into_iter().map(|x| x as f32).collect(),
        DecodingResult::F32(v) => v,
        DecodingResult::F64(v) => v.into_iter().map(|x| x as f32).collect(),
        _ => return Err(Error::tiff(ctx, "unsupported sample type")),
    };
    let nd = h
        .nodata
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v as f32);
    let data = data
        .into_iter()
        .map(|v| {
            if !v.is_finite() || nd == Some(v) {
                f32::NAN
            } else {
                v
            }
        })
        .collect::<Vec<f32>>();
    if data.len() != h.width * h.height {
        return Err(Error::tiff(ctx, "sample count does not match the size"));
    }
    Ok(AnyRaster {
        width: h.width,
        height: h.height,
        transform: h.transform,
        crs: h.crs.ok(),
        epsg: h.epsg,
        data,
    })
}

/// Reads a single-band GeoTIFF file of any common sample type ([`decode_any`]).
pub fn read_any(path: &Path) -> Result<AnyRaster> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    decode_any(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_values_georeferencing_and_nan() {
        let gt = GeoTransform::north_up(997_999.75, 6_426_000.25, 5.0);
        let data = vec![1.5, f32::NAN, -3.25, 2000.0, 0.0, 7.0];
        let g = Grid::from_vec(3, 2, gt, Crs::Lambert93, data.clone()).unwrap();
        let mut buf = Cursor::new(Vec::new());
        encode_f32(&mut buf, &g).unwrap();
        let back = decode_f32(buf.get_ref()).unwrap();
        assert_eq!(back.width(), 3);
        assert_eq!(back.height(), 2);
        assert_eq!(back.transform(), &gt);
        assert_eq!(back.crs(), Crs::Lambert93);
        for (a, b) in back.data().iter().zip(&data) {
            assert!(a == b || (a.is_nan() && b.is_nan()), "{a} vs {b}");
        }
    }

    /// A UInt8 GeoTIFF with the given projected EPSG code (written as GDAL does).
    fn u8_tiff(epsg: u16, data: &[u8]) -> Vec<u8> {
        use tiff::encoder::colortype::Gray8;
        let mut buf = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut buf).unwrap();
        let mut img = enc.new_image::<Gray8>(3, 2).unwrap();
        let d = img.encoder();
        d.write_tag(Tag::ModelPixelScaleTag, &[5.0, 5.0, 0.0][..])
            .unwrap();
        d.write_tag(
            Tag::ModelTiepointTag,
            &[0.0, 0.0, 0.0, 988_709.75, 6_431_980.25, 0.0][..],
        )
        .unwrap();
        let mut keys = geo_keys(Crs::Lambert93);
        let i = keys.iter().position(|k| *k == 2154).unwrap();
        keys[i] = epsg;
        d.write_tag(Tag::GeoKeyDirectoryTag, &keys[..]).unwrap();
        d.write_tag(Tag::GdalNodata, "255").unwrap();
        img.write_data(data).unwrap();
        buf.into_inner()
    }

    #[test]
    fn u8_and_any_sample_type() {
        let data = [1u8, 2, 3, 255, 0, 2];
        let b = u8_tiff(2154, &data);
        let g = decode_u8(&b).unwrap();
        assert_eq!(g.data(), &data);
        assert_eq!(g.nodata(), Some(&255));
        assert_eq!(g.crs(), Crs::Lambert93);
        assert_eq!(g.transform().origin_x, 988_709.75);
        assert!(decode_f32(&b).is_err(), "not Float32");
        let a = decode_any(&b).unwrap();
        assert_eq!(
            (a.width, a.height, a.epsg, a.crs),
            (3, 2, Some(2154), Some(Crs::Lambert93))
        );
        assert_eq!(a.data[..3], [1.0, 2.0, 3.0]);
        assert!(a.data[3].is_nan());
        // A CRS this crate does not handle (UTM 11N, the AutoATES workshop data): the
        // lenient reader keeps the code, the typed readers refuse.
        let utm = u8_tiff(32611, &data);
        let a = decode_any(&utm).unwrap();
        assert_eq!((a.epsg, a.crs), (Some(32611), None));
        assert!(decode_u8(&utm).is_err());
        // Float32 files through the lenient reader.
        let gt = GeoTransform::north_up(1000.0, 2000.0, 10.0);
        let f = Grid::from_vec(2, 1, gt, Crs::Lambert93, vec![1.5f32, f32::NAN]).unwrap();
        let mut buf = Cursor::new(Vec::new());
        encode_f32(&mut buf, &f).unwrap();
        let a = decode_any(buf.get_ref()).unwrap();
        assert_eq!(a.data[0], 1.5);
        assert!(a.data[1].is_nan());
        assert_eq!(a.transform, gt);
    }

    #[test]
    fn geographic_keys_round_trip() {
        let keys = geo_keys(Crs::Wgs84);
        assert_eq!(crs_from_keys(&keys, &[], "").unwrap(), Crs::Wgs84);
        assert_eq!(
            crs_from_keys(&geo_keys(Crs::Lambert93), &[], "").unwrap(),
            Crs::Lambert93
        );
        assert!(crs_from_keys(&[1, 1, 0, 0], &[], "").is_err());
    }

    /// GeoKey directory written by the Géoplateforme WMS-R for LiDAR HD tiles (dumped
    /// from tile 0988-6432): user-defined LCC with the Lambert-93 parameters.
    const IGN_KEYS: [u16; 80] = [
        1, 1, 0, 19, 1024, 0, 1, 1, 1025, 0, 1, 1, 1026, 34737, 10, 0, 2048, 0, 1, 32767, 2050, 0,
        1, 32767, 2051, 0, 1, 32767, 2052, 0, 1, 9001, 2054, 0, 1, 9102, 2056, 0, 1, 32767, 3072,
        0, 1, 32767, 3074, 0, 1, 32767, 3075, 0, 1, 8, 3076, 0, 1, 9001, 3078, 34736, 1, 0, 3079,
        34736, 1, 1, 3084, 34736, 1, 2, 3085, 34736, 1, 3, 3086, 34736, 1, 4, 3087, 34736, 1, 5,
    ];
    const IGN_DOUBLES: [f64; 6] = [49.0, 44.0, 3.0, 46.5, 700_000.0, 6_600_000.0];

    #[test]
    fn ign_user_defined_lambert93() {
        assert_eq!(
            crs_from_keys(&IGN_KEYS, &IGN_DOUBLES, "EPSG:2154|\0").unwrap(),
            Crs::Lambert93
        );
        // Same projection without the citation, or with other parameters: rejected.
        assert!(crs_from_keys(&IGN_KEYS, &IGN_DOUBLES, "").is_err());
        let mut other = IGN_DOUBLES;
        other[0] = 45.0;
        assert!(crs_from_keys(&IGN_KEYS, &other, "EPSG:2154|").is_err());
    }

    #[test]
    fn rejects_non_tiff() {
        assert!(decode_f32(b"<?xml version=\"1.0\"?><ServiceExceptionReport/>").is_err());
    }
}
