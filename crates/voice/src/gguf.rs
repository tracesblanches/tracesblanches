//! Minimal GGUF (v2, v3) metadata reader: the key-value header only, no tensors.
//!
//! Used to read the tokenizer and the text preparation rules of a Pocket TTS pack, exactly
//! as pockettts.cpp loads them.

use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use std::path::Path;

/// A metadata value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Any unsigned integer type.
    U64(u64),
    /// Any signed integer type.
    I64(i64),
    /// f32 or f64.
    F64(f64),
    /// Boolean.
    Bool(bool),
    /// UTF-8 string.
    Str(String),
    /// Array of strings.
    StrArray(Vec<String>),
    /// Array of f32 (kept as f32: tokenizer scores are summed in f32).
    F32Array(Vec<f32>),
    /// Any other array, by element count.
    OtherArray(usize),
}

/// Metadata of a GGUF file.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    /// Key-value pairs.
    pub kv: BTreeMap<String, Value>,
}

/// Error reading a GGUF file.
#[derive(Debug, thiserror::Error)]
#[error("GGUF {path}: {message}")]
pub struct GgufError {
    /// File.
    pub path: String,
    /// What went wrong.
    pub message: String,
}

impl Metadata {
    /// Reads the metadata of the GGUF file at `path`.
    pub fn read(path: &Path) -> Result<Self, GgufError> {
        let err = |m: String| GgufError {
            path: path.display().to_string(),
            message: m,
        };
        let f = std::fs::File::open(path).map_err(|e| err(e.to_string()))?;
        Self::parse(&mut BufReader::new(f)).map_err(err)
    }

    /// Parses metadata from a reader positioned at the start of the file.
    pub fn parse(r: &mut impl Read) -> Result<Self, String> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic).map_err(|e| e.to_string())?;
        if &magic != b"GGUF" {
            return Err("not a GGUF file".into());
        }
        let version = u32_(r)?;
        if !(2..=3).contains(&version) {
            return Err(format!("unsupported GGUF version {version}"));
        }
        let _n_tensors = u64_(r)?;
        let n_kv = u64_(r)?;
        let mut kv = BTreeMap::new();
        for _ in 0..n_kv {
            let key = string(r)?;
            let ty = u32_(r)?;
            let v = value(r, ty)?;
            kv.insert(key, v);
        }
        Ok(Self { kv })
    }

    /// Boolean value of `key`.
    pub fn bool(&self, key: &str) -> Result<bool, String> {
        match self.kv.get(key) {
            Some(Value::Bool(b)) => Ok(*b),
            other => Err(format!("{key}: expected a bool, found {other:?}")),
        }
    }

    /// String array value of `key` (an absent key is an empty array).
    pub fn strings(&self, key: &str) -> Result<Vec<String>, String> {
        match self.kv.get(key) {
            Some(Value::StrArray(v)) => Ok(v.clone()),
            None => Ok(Vec::new()),
            other => Err(format!("{key}: expected a string array, found {other:?}")),
        }
    }

    /// f32 array value of `key`.
    pub fn f32s(&self, key: &str) -> Result<Vec<f32>, String> {
        match self.kv.get(key) {
            Some(Value::F32Array(v)) => Ok(v.clone()),
            other => Err(format!("{key}: expected an f32 array, found {other:?}")),
        }
    }
}

fn bytes<const N: usize>(r: &mut impl Read) -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn u32_(r: &mut impl Read) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes::<4>(r)?))
}

fn u64_(r: &mut impl Read) -> Result<u64, String> {
    Ok(u64::from_le_bytes(bytes::<8>(r)?))
}

fn string(r: &mut impl Read) -> Result<String, String> {
    let n = u64_(r)?;
    if n > 1 << 30 {
        return Err(format!("string of {n} bytes"));
    }
    let mut b = vec![0u8; n as usize];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    String::from_utf8(b).map_err(|e| e.to_string())
}

fn scalar(r: &mut impl Read, ty: u32) -> Result<Value, String> {
    Ok(match ty {
        0 => Value::U64(u64::from(bytes::<1>(r)?[0])),
        1 => Value::I64(i64::from(i8::from_le_bytes(bytes::<1>(r)?))),
        2 => Value::U64(u64::from(u16::from_le_bytes(bytes::<2>(r)?))),
        3 => Value::I64(i64::from(i16::from_le_bytes(bytes::<2>(r)?))),
        4 => Value::U64(u64::from(u32_(r)?)),
        5 => Value::I64(i64::from(i32::from_le_bytes(bytes::<4>(r)?))),
        6 => Value::F64(f64::from(f32::from_le_bytes(bytes::<4>(r)?))),
        7 => Value::Bool(bytes::<1>(r)?[0] != 0),
        8 => Value::Str(string(r)?),
        10 => Value::U64(u64_(r)?),
        11 => Value::I64(i64::from_le_bytes(bytes::<8>(r)?)),
        12 => Value::F64(f64::from_le_bytes(bytes::<8>(r)?)),
        t => return Err(format!("unknown value type {t}")),
    })
}

fn value(r: &mut impl Read, ty: u32) -> Result<Value, String> {
    if ty != 9 {
        return scalar(r, ty);
    }
    let et = u32_(r)?;
    let n = u64_(r)?;
    if n > 1 << 28 {
        return Err(format!("array of {n} elements"));
    }
    let n = n as usize;
    match et {
        8 => (0..n)
            .map(|_| string(r))
            .collect::<Result<_, _>>()
            .map(Value::StrArray),
        6 => (0..n)
            .map(|_| bytes::<4>(r).map(f32::from_le_bytes))
            .collect::<Result<_, _>>()
            .map(Value::F32Array),
        9 => Err("nested arrays are not supported".into()),
        t => {
            for _ in 0..n {
                scalar(r, t)?;
            }
            Ok(Value::OtherArray(n))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_str(b: &mut Vec<u8>, s: &str) {
        b.extend((s.len() as u64).to_le_bytes());
        b.extend(s.as_bytes());
    }

    #[test]
    fn parses_a_synthetic_header() {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(4u64.to_le_bytes());
        put_str(&mut b, "a.bool");
        b.extend(7u32.to_le_bytes());
        b.push(1);
        put_str(&mut b, "a.strs");
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(2u64.to_le_bytes());
        put_str(&mut b, "x");
        put_str(&mut b, "");
        put_str(&mut b, "a.f32s");
        b.extend(9u32.to_le_bytes());
        b.extend(6u32.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend((-1.5f32).to_le_bytes());
        put_str(&mut b, "a.u32");
        b.extend(4u32.to_le_bytes());
        b.extend(7u32.to_le_bytes());
        let m = Metadata::parse(&mut b.as_slice()).unwrap();
        assert!(m.bool("a.bool").unwrap());
        assert_eq!(m.strings("a.strs").unwrap(), ["x", ""]);
        assert_eq!(m.f32s("a.f32s").unwrap(), [-1.5]);
        assert_eq!(m.kv["a.u32"], Value::U64(7));
        assert!(m.strings("missing").unwrap().is_empty());
        assert!(Metadata::parse(&mut &b"GGUX"[..]).is_err());
    }
}
