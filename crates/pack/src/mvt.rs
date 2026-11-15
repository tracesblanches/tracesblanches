//! Minimal Mapbox Vector Tile 2.1 writer and reader, for the polygon class layers of the
//! pack (slope, avalanche exposure: [`crate::classes`]).
//!
//! Only what these layers need: polygon features (exterior rings with positive area in
//! tile coordinates, y down, interior rings negative), string and integer attributes.
//! The reader decodes every geometry type's commands but the class layers only hold
//! polygons.

use std::collections::HashMap;

/// Tile extent of the class layers (MVT default).
pub const EXTENT: u32 = 4096;

/// An attribute value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `string_value`.
    Str(String),
    /// `int64_value` (written as `sint64`, read from any integer field).
    Int(i64),
    /// `double_value` (or `float_value` when read).
    Double(f64),
    /// `bool_value`.
    Bool(bool),
}

impl Value {
    /// Integer value, if the value is a number without fraction.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Double(d) if d.fract() == 0.0 => Some(*d as i64),
            _ => None,
        }
    }
}

/// A ring: integer tile coordinates, not closed (the last point is not the first).
pub type Ring = Vec<[i32; 2]>;

/// Signed area (surveyor's formula) of a ring in tile coordinates: positive for an
/// exterior ring of the MVT specification (clockwise on screen, y down).
pub fn ring_area(r: &[[i32; 2]]) -> f64 {
    let n = r.len();
    let mut a = 0.0;
    for i in 0..n {
        let [x0, y0] = r[i];
        let [x1, y1] = r[(i + 1) % n];
        a += f64::from(x0) * f64::from(y1) - f64::from(x1) * f64::from(y0);
    }
    a / 2.0
}

// ---------------------------------------------------------------------------------------
// Writer.

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn key(out: &mut Vec<u8>, field: u32, wire: u8) {
    varint(out, (u64::from(field) << 3) | u64::from(wire));
}

fn bytes_field(out: &mut Vec<u8>, field: u32, b: &[u8]) {
    key(out, field, 2);
    varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

fn command(id: u32, count: u32) -> u32 {
    (id & 0x7) | (count << 3)
}

/// One layer of a tile being written.
#[derive(Debug, Default)]
pub struct LayerWriter {
    name: String,
    keys: Vec<String>,
    key_index: HashMap<String, u32>,
    values: Vec<Vec<u8>>,
    value_index: HashMap<Vec<u8>, u32>,
    features: Vec<Vec<u8>>,
}

impl LayerWriter {
    /// A layer named `name` (the `source-layer` of a style).
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    /// Number of features written.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Whether no feature was written.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    fn key_id(&mut self, k: &str) -> u32 {
        if let Some(&i) = self.key_index.get(k) {
            return i;
        }
        let i = self.keys.len() as u32;
        self.keys.push(k.to_owned());
        self.key_index.insert(k.to_owned(), i);
        i
    }

    fn value_id(&mut self, v: &Value) -> u32 {
        let mut b = Vec::new();
        match v {
            Value::Str(s) => bytes_field(&mut b, 1, s.as_bytes()),
            Value::Double(d) => {
                key(&mut b, 3, 1);
                b.extend_from_slice(&d.to_le_bytes());
            }
            Value::Int(i) => {
                key(&mut b, 6, 0);
                varint(&mut b, ((i << 1) ^ (i >> 63)) as u64);
            }
            Value::Bool(x) => {
                key(&mut b, 7, 0);
                varint(&mut b, u64::from(*x));
            }
        }
        if let Some(&i) = self.value_index.get(&b) {
            return i;
        }
        let i = self.values.len() as u32;
        self.values.push(b.clone());
        self.value_index.insert(b, i);
        i
    }

    /// Adds a polygon feature made of `rings` (each exterior followed by its holes,
    /// oriented as the specification says; see [`ring_area`]). Rings of fewer than three
    /// points are skipped; nothing is written if no ring is left.
    pub fn add_polygon(&mut self, props: &[(&str, Value)], rings: &[Ring]) {
        let mut geom: Vec<u32> = Vec::new();
        let (mut cx, mut cy) = (0i32, 0i32);
        for r in rings.iter().filter(|r| r.len() >= 3) {
            geom.push(command(1, 1));
            geom.push(zigzag(r[0][0] - cx));
            geom.push(zigzag(r[0][1] - cy));
            (cx, cy) = (r[0][0], r[0][1]);
            geom.push(command(2, (r.len() - 1) as u32));
            for p in &r[1..] {
                geom.push(zigzag(p[0] - cx));
                geom.push(zigzag(p[1] - cy));
                (cx, cy) = (p[0], p[1]);
            }
            geom.push(command(7, 1));
        }
        if geom.is_empty() {
            return;
        }
        let mut tags = Vec::with_capacity(props.len() * 2);
        for (k, v) in props {
            tags.push(self.key_id(k));
            tags.push(self.value_id(v));
        }
        let mut f = Vec::new();
        let mut packed = Vec::new();
        for t in tags {
            varint(&mut packed, u64::from(t));
        }
        bytes_field(&mut f, 2, &packed);
        key(&mut f, 3, 0);
        varint(&mut f, 3); // POLYGON
        packed.clear();
        for g in geom {
            varint(&mut packed, u64::from(g));
        }
        bytes_field(&mut f, 4, &packed);
        self.features.push(f);
    }

    fn encode(&self, extent: u32) -> Vec<u8> {
        let mut l = Vec::new();
        key(&mut l, 15, 0);
        varint(&mut l, 2);
        bytes_field(&mut l, 1, self.name.as_bytes());
        for f in &self.features {
            bytes_field(&mut l, 2, f);
        }
        for k in &self.keys {
            bytes_field(&mut l, 3, k.as_bytes());
        }
        for v in &self.values {
            bytes_field(&mut l, 4, v);
        }
        key(&mut l, 5, 0);
        varint(&mut l, u64::from(extent));
        l
    }
}

/// Encodes a tile of layers (empty layers are left out); `None` if every layer is empty.
pub fn encode_tile(layers: &[LayerWriter], extent: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for l in layers.iter().filter(|l| !l.is_empty()) {
        bytes_field(&mut out, 3, &l.encode(extent));
    }
    (!out.is_empty()).then_some(out)
}

// ---------------------------------------------------------------------------------------
// Reader.

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
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .i
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or(DecodeError("truncated field"))?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.varint()? as usize;
        self.take(n)
    }
    /// Next `(field, wire type)`.
    fn key(&mut self) -> Result<(u32, u8), DecodeError> {
        let k = self.varint()?;
        Ok(((k >> 3) as u32, (k & 7) as u8))
    }
    fn skip(&mut self, wire: u8) -> Result<(), DecodeError> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => {
                self.take(8)?;
            }
            2 => {
                self.bytes()?;
            }
            5 => {
                self.take(4)?;
            }
            _ => return Err(DecodeError("unsupported wire type")),
        }
        Ok(())
    }
    fn packed(&mut self) -> Result<Vec<u32>, DecodeError> {
        let mut r = Reader::new(self.bytes()?);
        let mut out = Vec::new();
        while !r.done() {
            out.push(r.varint()? as u32);
        }
        Ok(out)
    }
}

/// A decoded feature.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Geometry type: 1 point, 2 line, 3 polygon.
    pub geom_type: u8,
    /// Attributes.
    pub props: Vec<(String, Value)>,
    /// Rings (polygons) or lines and points, in tile coordinates.
    pub parts: Vec<Ring>,
}

impl Feature {
    /// Attribute by key.
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.props.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }
}

/// A decoded layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Name.
    pub name: String,
    /// Extent.
    pub extent: u32,
    /// Features.
    pub features: Vec<Feature>,
}

fn decode_value(b: &[u8]) -> Result<Value, DecodeError> {
    let mut r = Reader::new(b);
    let mut out = Value::Bool(false);
    while !r.done() {
        let (field, wire) = r.key()?;
        out = match (field, wire) {
            (1, 2) => Value::Str(
                String::from_utf8(r.bytes()?.to_vec()).map_err(|_| DecodeError("utf-8"))?,
            ),
            (2, 5) => {
                let b = r.take(4)?;
                Value::Double(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            }
            (3, 1) => {
                let b = r.take(8)?;
                let mut a = [0u8; 8];
                a.copy_from_slice(b);
                Value::Double(f64::from_le_bytes(a))
            }
            (4, 0) => Value::Int(r.varint()? as i64),
            (5, 0) => Value::Int(r.varint()? as i64),
            (6, 0) => {
                let v = r.varint()?;
                Value::Int(((v >> 1) as i64) ^ -((v & 1) as i64))
            }
            (7, 0) => Value::Bool(r.varint()? != 0),
            (_, w) => {
                r.skip(w)?;
                continue;
            }
        };
    }
    Ok(out)
}

fn decode_geometry(cmds: &[u32]) -> Result<Vec<Ring>, DecodeError> {
    let mut parts: Vec<Ring> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    let mut i = 0;
    let unzig = |v: u32| ((v >> 1) as i32) ^ -((v & 1) as i32);
    while i < cmds.len() {
        let (id, count) = (cmds[i] & 7, cmds[i] >> 3);
        i += 1;
        match id {
            1 | 2 => {
                for _ in 0..count {
                    let (dx, dy) = (
                        *cmds.get(i).ok_or(DecodeError("truncated geometry"))?,
                        *cmds.get(i + 1).ok_or(DecodeError("truncated geometry"))?,
                    );
                    i += 2;
                    x = x.wrapping_add(unzig(dx));
                    y = y.wrapping_add(unzig(dy));
                    if id == 1 {
                        parts.push(vec![[x, y]]);
                    } else {
                        parts
                            .last_mut()
                            .ok_or(DecodeError("LineTo before MoveTo"))?
                            .push([x, y]);
                    }
                }
            }
            7 => {}
            _ => return Err(DecodeError("unknown geometry command")),
        }
    }
    Ok(parts)
}

/// Decodes a tile (not compressed).
pub fn decode(tile: &[u8]) -> Result<Vec<Layer>, DecodeError> {
    let mut r = Reader::new(tile);
    let mut layers = Vec::new();
    while !r.done() {
        let (field, wire) = r.key()?;
        if field != 3 || wire != 2 {
            r.skip(wire)?;
            continue;
        }
        let mut lr = Reader::new(r.bytes()?);
        let mut name = String::new();
        let mut extent = EXTENT;
        let mut keys = Vec::new();
        let mut values = Vec::new();
        let mut raw = Vec::new();
        while !lr.done() {
            let (f, w) = lr.key()?;
            match (f, w) {
                (1, 2) => {
                    name = String::from_utf8(lr.bytes()?.to_vec())
                        .map_err(|_| DecodeError("utf-8"))?;
                }
                (2, 2) => raw.push(lr.bytes()?),
                (3, 2) => keys.push(
                    String::from_utf8(lr.bytes()?.to_vec()).map_err(|_| DecodeError("utf-8"))?,
                ),
                (4, 2) => values.push(decode_value(lr.bytes()?)?),
                (5, 0) => extent = lr.varint()? as u32,
                (_, w) => lr.skip(w)?,
            }
        }
        let mut features = Vec::new();
        for fb in raw {
            let mut fr = Reader::new(fb);
            let (mut tags, mut geom, mut ty) = (Vec::new(), Vec::new(), 0u8);
            while !fr.done() {
                let (f, w) = fr.key()?;
                match (f, w) {
                    (2, 2) => tags = fr.packed()?,
                    (3, 0) => ty = fr.varint()? as u8,
                    (4, 2) => geom = fr.packed()?,
                    (_, w) => fr.skip(w)?,
                }
            }
            let props = tags
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[ki, vi]| {
                    let k = keys.get(*ki as usize).ok_or(DecodeError("bad key"))?;
                    let v = values.get(*vi as usize).ok_or(DecodeError("bad value"))?;
                    Ok((k.clone(), v.clone()))
                })
                .collect::<Result<_, DecodeError>>()?;
            features.push(Feature {
                geom_type: ty,
                props,
                parts: decode_geometry(&geom)?,
            });
        }
        layers.push(Layer {
            name,
            extent,
            features,
        });
    }
    Ok(layers)
}

/// Fills the pixels whose centre is inside `rings` (even-odd rule over all the rings, so
/// that holes stay empty), rings in pixel coordinates of a `w × h` image. Calls
/// `set(index)` for each pixel filled.
pub fn fill_even_odd(rings: &[Vec<[f64; 2]>], w: usize, h: usize, mut set: impl FnMut(usize)) {
    // Edge table: the crossings of every edge with the pixel-centre lines it spans, by
    // row (half-open rule: an edge counts when the centre line is in [min, max)).
    let mut rows: Vec<Vec<f64>> = vec![Vec::new(); h];
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let [x0, y0] = r[i];
            let [x1, y1] = r[(i + 1) % n];
            if y0 == y1 {
                continue;
            }
            let (lo, hi) = (y0.min(y1), y0.max(y1));
            let first = (lo - 0.5).ceil().max(0.0) as usize;
            let last = ((hi - 0.5).ceil().max(0.0) as usize).min(h);
            for (row, xs) in rows.iter_mut().enumerate().take(last).skip(first) {
                let yc = row as f64 + 0.5;
                if (y0 <= yc) != (y1 <= yc) {
                    xs.push(x0 + (yc - y0) * (x1 - x0) / (y1 - y0));
                }
            }
        }
    }
    for (row, xs) in rows.iter_mut().enumerate() {
        xs.sort_by(f64::total_cmp);
        for [x0, x1] in xs.as_chunks::<2>().0 {
            // Pixels whose centre x is in [x0, x1).
            let a = (x0 - 0.5).ceil().max(0.0) as usize;
            let b = ((x1 - 0.5).ceil().max(0.0) as usize).min(w);
            for col in a..b {
                set(row * w + col);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut l = LayerWriter::new("slope");
        let outer: Ring = vec![[0, 0], [100, 0], [100, 100], [0, 100]];
        let hole: Ring = vec![[20, 20], [20, 80], [80, 80], [80, 20]];
        assert!(ring_area(&outer) > 0.0 && ring_area(&hole) < 0.0);
        l.add_polygon(
            &[
                ("class", Value::Int(2)),
                ("label", Value::Str("35 à 40°".into())),
            ],
            &[outer.clone(), hole.clone()],
        );
        l.add_polygon(&[("class", Value::Int(-3))], std::slice::from_ref(&outer));
        // Degenerate ring only: no feature.
        l.add_polygon(&[("class", Value::Int(1))], &[vec![[0, 0], [1, 1]]]);
        assert_eq!(l.len(), 2);
        let bytes = encode_tile(&[l, LayerWriter::new("empty")], EXTENT).unwrap();
        let layers = decode(&bytes).unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].name, "slope");
        assert_eq!(layers[0].extent, 4096);
        let f = &layers[0].features[0];
        assert_eq!(f.geom_type, 3);
        assert_eq!(f.get("class").and_then(Value::as_i64), Some(2));
        assert_eq!(f.get("label"), Some(&Value::Str("35 à 40°".into())));
        assert_eq!(f.parts, vec![outer.clone(), hole]);
        assert_eq!(
            layers[0].features[1].get("class").and_then(Value::as_i64),
            Some(-3)
        );
        assert!(encode_tile(&[LayerWriter::new("x")], EXTENT).is_none());
    }

    #[test]
    fn even_odd_fill_keeps_holes() {
        let sq = |a: f64, b: f64| vec![[a, a], [b, a], [b, b], [a, b]];
        let mut img = [0u8; 100];
        fill_even_odd(&[sq(0.0, 10.0), sq(3.0, 7.0)], 10, 10, |i| img[i] = 1);
        let filled = img.iter().filter(|&&v| v == 1).count();
        assert_eq!(filled, 100 - 16);
        assert_eq!(img[5 * 10 + 5], 0);
        assert_eq!(img[0], 1);
        // Centres only: a square from 0.6 to 2.4 covers the pixels of centres 1.5 and 2.5?
        // 2.5 is outside [0.6, 2.4): only centre 1.5 on each axis.
        let mut img = [0u8; 16];
        fill_even_odd(&[sq(0.6, 2.4)], 4, 4, |i| img[i] = 1);
        assert_eq!(img.iter().filter(|&&v| v == 1).count(), 1);
        assert_eq!(img[4 + 1], 1);
    }
}
