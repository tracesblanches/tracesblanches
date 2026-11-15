//! Minimal Mapbox Vector Tile (MVT 2.1) decoder: layers, keys, values, feature tags and
//! geometry types. Geometries are skipped: the validator only needs attributes.

use std::fmt;

/// An attribute value of a vector tile feature.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `string_value`.
    Str(String),
    /// `float_value`, `double_value`, `int_value`, `uint_value`, `sint_value`.
    Num(f64),
    /// `bool_value`.
    Bool(bool),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Num(n) => write!(f, "{n}"),
            Value::Bool(b) => write!(f, "{b}"),
        }
    }
}

/// Geometry type of a feature (MVT `GeomType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GeomType {
    /// `UNKNOWN` (0).
    Unknown,
    /// `POINT` (1).
    Point,
    /// `LINESTRING` (2).
    LineString,
    /// `POLYGON` (3).
    Polygon,
}

impl GeomType {
    /// MapLibre `geometry-type` name.
    pub fn name(self) -> &'static str {
        match self {
            GeomType::Unknown => "Unknown",
            GeomType::Point => "Point",
            GeomType::LineString => "LineString",
            GeomType::Polygon => "Polygon",
        }
    }
}

/// A feature: geometry type and attributes (indices into the layer keys and values).
#[derive(Debug, Clone)]
pub struct Feature {
    /// Geometry type.
    pub geom: GeomType,
    /// `(key index, value index)` pairs.
    pub tags: Vec<(u32, u32)>,
}

/// A layer of a tile.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Layer name (`source-layer` of a style).
    pub name: String,
    /// Keys table.
    pub keys: Vec<String>,
    /// Values table.
    pub values: Vec<Value>,
    /// Features.
    pub features: Vec<Feature>,
}

impl Layer {
    /// Attributes of a feature as `(key, value)`; tags pointing outside the tables are skipped.
    pub fn attributes<'a>(&'a self, f: &'a Feature) -> impl Iterator<Item = (&'a str, &'a Value)> {
        f.tags.iter().filter_map(|&(k, v)| {
            Some((
                self.keys.get(k as usize)?.as_str(),
                self.values.get(v as usize)?,
            ))
        })
    }
}

/// Decoding error.
#[derive(Debug, thiserror::Error)]
#[error("invalid vector tile: {0}")]
pub struct DecodeError(&'static str);

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }
    fn done(&self) -> bool {
        self.i >= self.b.len()
    }
    fn varint(&mut self) -> Result<u64, DecodeError> {
        let mut out = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.b.get(self.i).ok_or(DecodeError("truncated varint"))?;
            self.i += 1;
            out |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(out);
            }
        }
        Err(DecodeError("varint too long"))
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .i
            .checked_add(n)
            .ok_or(DecodeError("length overflow"))?;
        let s = self
            .b
            .get(self.i..end)
            .ok_or(DecodeError("truncated field"))?;
        self.i = end;
        Ok(s)
    }
    fn len_delimited(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = usize::try_from(self.varint()?).map_err(|_| DecodeError("length overflow"))?;
        self.bytes(n)
    }
    /// Next `(field number, wire type)`.
    fn key(&mut self) -> Result<(u64, u8), DecodeError> {
        let k = self.varint()?;
        Ok((k >> 3, (k & 7) as u8))
    }
    fn skip(&mut self, wire: u8) -> Result<(), DecodeError> {
        match wire {
            0 => self.varint().map(drop),
            1 => self.bytes(8).map(drop),
            2 => self.len_delimited().map(drop),
            5 => self.bytes(4).map(drop),
            _ => Err(DecodeError("unsupported wire type")),
        }
    }
}

/// Decodes an uncompressed vector tile.
pub fn decode(tile: &[u8]) -> Result<Vec<Layer>, DecodeError> {
    let mut r = Reader::new(tile);
    let mut layers = Vec::new();
    while !r.done() {
        let (field, wire) = r.key()?;
        if field == 3 && wire == 2 {
            layers.push(decode_layer(r.len_delimited()?)?);
        } else {
            r.skip(wire)?;
        }
    }
    Ok(layers)
}

fn decode_layer(b: &[u8]) -> Result<Layer, DecodeError> {
    let mut r = Reader::new(b);
    let mut layer = Layer {
        name: String::new(),
        keys: Vec::new(),
        values: Vec::new(),
        features: Vec::new(),
    };
    while !r.done() {
        match r.key()? {
            (1, 2) => layer.name = utf8(r.len_delimited()?)?,
            (2, 2) => layer.features.push(decode_feature(r.len_delimited()?)?),
            (3, 2) => layer.keys.push(utf8(r.len_delimited()?)?),
            (4, 2) => layer.values.push(decode_value(r.len_delimited()?)?),
            (_, wire) => r.skip(wire)?,
        }
    }
    Ok(layer)
}

fn decode_feature(b: &[u8]) -> Result<Feature, DecodeError> {
    let mut r = Reader::new(b);
    let mut f = Feature {
        geom: GeomType::Unknown,
        tags: Vec::new(),
    };
    while !r.done() {
        match r.key()? {
            (2, 2) => {
                let mut p = Reader::new(r.len_delimited()?);
                while !p.done() {
                    let k = p.varint()?;
                    let v = p.varint()?;
                    let k = u32::try_from(k).map_err(|_| DecodeError("tag overflow"))?;
                    let v = u32::try_from(v).map_err(|_| DecodeError("tag overflow"))?;
                    f.tags.push((k, v));
                }
            }
            (3, 0) => {
                f.geom = match r.varint()? {
                    1 => GeomType::Point,
                    2 => GeomType::LineString,
                    3 => GeomType::Polygon,
                    _ => GeomType::Unknown,
                }
            }
            (_, wire) => r.skip(wire)?,
        }
    }
    Ok(f)
}

fn decode_value(b: &[u8]) -> Result<Value, DecodeError> {
    let mut r = Reader::new(b);
    let mut out = Value::Str(String::new());
    while !r.done() {
        out = match r.key()? {
            (1, 2) => Value::Str(utf8(r.len_delimited()?)?),
            (2, 5) => {
                let s = r.bytes(4)?;
                Value::Num(f64::from(f32::from_le_bytes([s[0], s[1], s[2], s[3]])))
            }
            (3, 1) => {
                let s = r.bytes(8)?;
                let mut a = [0u8; 8];
                a.copy_from_slice(s);
                Value::Num(f64::from_le_bytes(a))
            }
            // int64 (two's complement in a varint) and uint64.
            (4, 0) => Value::Num(r.varint()?.cast_signed() as f64),
            (5, 0) => Value::Num(r.varint()? as f64),
            (6, 0) => {
                let n = r.varint()?;
                Value::Num(((n >> 1).cast_signed() ^ -((n & 1).cast_signed())) as f64)
            }
            (7, 0) => Value::Bool(r.varint()? != 0),
            (_, wire) => {
                r.skip(wire)?;
                continue;
            }
        };
    }
    Ok(out)
}

fn utf8(b: &[u8]) -> Result<String, DecodeError> {
    String::from_utf8(b.to_vec()).map_err(|_| DecodeError("invalid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut n: u64, out: &mut Vec<u8>) {
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    fn field(num: u64, payload: &[u8], out: &mut Vec<u8>) {
        varint(num << 3 | 2, out);
        varint(payload.len() as u64, out);
        out.extend_from_slice(payload);
    }

    #[test]
    fn decodes_a_hand_made_tile() {
        // value 0: "forêt" ; value 1: sint -3 ; value 2: true
        let mut v0 = Vec::new();
        field(1, "forêt".as_bytes(), &mut v0);
        let v1 = vec![6 << 3, 5]; // zigzag(-3) = 5
        let v2 = vec![7 << 3, 1];
        let mut feat = Vec::new();
        let mut tags = Vec::new();
        for t in [0u64, 0, 1, 1, 2, 2] {
            varint(t, &mut tags);
        }
        field(2, &tags, &mut feat);
        feat.extend_from_slice(&[3 << 3, 3]); // polygon
        let mut layer = Vec::new();
        layer.extend_from_slice(&[15 << 3, 2]); // version 2
        field(1, b"ocs_vegetation_surf", &mut layer);
        field(2, &feat, &mut layer);
        for k in ["nature", "n", "b"] {
            field(3, k.as_bytes(), &mut layer);
        }
        for v in [&v0, &v1, &v2] {
            field(4, v, &mut layer);
        }
        let mut tile = Vec::new();
        field(3, &layer, &mut tile);
        let layers = decode(&tile).unwrap();
        assert_eq!(layers.len(), 1);
        let l = &layers[0];
        assert_eq!(l.name, "ocs_vegetation_surf");
        assert_eq!(l.features[0].geom, GeomType::Polygon);
        let attrs: Vec<_> = l.attributes(&l.features[0]).collect();
        assert_eq!(
            attrs,
            vec![
                ("nature", &Value::Str("forêt".into())),
                ("n", &Value::Num(-3.0)),
                ("b", &Value::Bool(true))
            ]
        );
    }

    #[test]
    fn rejects_truncated_tiles() {
        assert!(decode(&[3 << 3 | 2, 10, 1]).is_err());
    }
}
