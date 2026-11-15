//! Golden fingerprints (Q2, #102): key figures of a layer or of a computation, compared
//! with a versioned reference `fixtures/golden/<name>.json`.
//!
//! The reference holds values only (sorted keys, one per line: a change reads as a
//! one-line diff in a pull request); each test declares the tolerance of each value next
//! to the code that computes it, with its justification. A value out of tolerance, a new
//! key or a missing key fails the test with a table of the differences.
//!
//! Updating the references is explicit: `SKIMAP_BLESS=1 cargo test …` rewrites the
//! values that are out of tolerance (and adds or removes keys), prints the same table,
//! and leaves the values within tolerance untouched, so that `git diff fixtures/golden/`
//! shows only what really changed.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::Value;

/// Tolerance of a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tol {
    /// Equal (counts, identifiers, texts).
    Exact,
    /// `|now − golden| ≤ a`.
    Abs(f64),
    /// `|now − golden| ≤ r · |golden|`.
    Rel(f64),
}

impl Tol {
    /// Deterministic computation on fixed inputs: only floating-point noise (summation
    /// order, last-bit differences of `libm` between platforms) is accepted.
    pub const FLOAT: Tol = Tol::Rel(1e-9);

    fn accepts(self, golden: &Value, now: &Value) -> bool {
        match (self, golden.as_f64(), now.as_f64()) {
            (Tol::Exact, _, _) => golden == now,
            (Tol::Abs(a), Some(g), Some(n)) => (n - g).abs() <= a,
            (Tol::Rel(r), Some(g), Some(n)) => (n - g).abs() <= r * g.abs(),
            _ => golden == now,
        }
    }

    fn describe(self) -> String {
        match self {
            Tol::Exact => "exact".into(),
            Tol::Abs(a) => format!("±{a}"),
            Tol::Rel(r) if r >= 1e-4 => format!("±{} %", r * 100.0),
            Tol::Rel(r) => format!("±{r:e} relative"),
        }
    }
}

/// Fingerprint under construction.
pub struct Golden {
    name: String,
    values: BTreeMap<String, (Value, Tol)>,
}

impl Golden {
    /// Fingerprint stored in `fixtures/golden/<name>.json` (`name` may contain `/`).
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            values: BTreeMap::new(),
        }
    }

    fn put(&mut self, key: &str, v: Value, tol: Tol) -> &mut Self {
        let old = self.values.insert(key.to_owned(), (v, tol));
        assert!(old.is_none(), "golden {}: key {key} given twice", self.name);
        self
    }

    /// A number with its tolerance. Non-finite values are refused (no golden NaN).
    pub fn num(&mut self, key: &str, v: f64, tol: Tol) -> &mut Self {
        assert!(v.is_finite(), "golden {}: {key} = {v}", self.name);
        self.put(key, serde_json::json!(v), tol)
    }

    /// A count, exact.
    pub fn count(&mut self, key: &str, v: impl TryInto<i64>) -> &mut Self {
        let v: i64 = v
            .try_into()
            .unwrap_or_else(|_| panic!("golden {}: {key} out of range", self.name));
        self.put(key, serde_json::json!(v), Tol::Exact)
    }

    /// A text (generated sentence, identifier), exact.
    pub fn text(&mut self, key: &str, v: impl Into<String>) -> &mut Self {
        self.put(key, Value::String(v.into()), Tol::Exact)
    }

    /// A multi-line text (generated sentences), exact, stored one line per array item so
    /// that a changed line reads as a one-line diff.
    pub fn lines(&mut self, key: &str, v: &str) -> &mut Self {
        let lines = v.lines().map(|l| Value::String(l.to_owned())).collect();
        self.put(key, Value::Array(lines), Tol::Exact)
    }

    fn path(&self) -> PathBuf {
        super::root()
            .join("golden")
            .join(format!("{}.json", self.name))
    }

    /// Compares with the reference (or rewrites it with `SKIMAP_BLESS=1`).
    pub fn check(&self) {
        let bless = std::env::var("SKIMAP_BLESS").is_ok_and(|v| v == "1" || v == "true");
        self.check_at(&self.path(), bless);
    }

    /// [`Golden::check`] against the reference at `path`, blessing or not (tests of this
    /// module).
    pub fn check_at(&self, path: &std::path::Path, bless: bool) {
        let path = path.to_path_buf();
        let old_text = std::fs::read_to_string(&path).unwrap_or_default();
        let old: BTreeMap<String, Value> = if path.exists() {
            serde_json::from_str(&old_text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        } else if bless {
            BTreeMap::new()
        } else {
            panic!(
                "golden reference missing: {} (create it with SKIMAP_BLESS=1 and review it)",
                path.display()
            )
        };
        let mut table = String::new();
        let mut out_of_tolerance = 0;
        let mut kept: BTreeMap<String, Value> = BTreeMap::new();
        let mut unchanged: std::collections::BTreeSet<String> = Default::default();
        for (k, (now, tol)) in &self.values {
            match old.get(k) {
                Some(g) if tol.accepts(g, now) => {
                    kept.insert(k.clone(), g.clone());
                    unchanged.insert(k.clone());
                }
                Some(g) => {
                    out_of_tolerance += 1;
                    let _ = writeln!(table, "  {k}  (tolerance {})", tol.describe());
                    match (g, now) {
                        (Value::Array(a), Value::Array(b)) => {
                            // Generated sentences: only the lines that differ.
                            for i in 0..a.len().max(b.len()) {
                                let (x, y) = (a.get(i), b.get(i));
                                if x != y {
                                    let show = |v: Option<&Value>| {
                                        v.and_then(Value::as_str).unwrap_or("(none)").to_owned()
                                    };
                                    let _ = writeln!(
                                        table,
                                        "      line {}\n        golden {}\n        now    {}",
                                        i + 1,
                                        show(x),
                                        show(y)
                                    );
                                }
                            }
                        }
                        _ => {
                            let diff = match (g.as_f64(), now.as_f64()) {
                                (Some(a), Some(b)) if a != 0.0 => {
                                    format!("{:+} ({:+.3} %)", b - a, (b - a) / a.abs() * 100.0)
                                }
                                (Some(a), Some(b)) => format!("{:+}", b - a),
                                _ => "text changed".into(),
                            };
                            let _ = writeln!(
                                table,
                                "      golden {g}\n      now    {now}\n      diff   {diff}"
                            );
                        }
                    }
                    kept.insert(k.clone(), now.clone());
                }
                None => {
                    out_of_tolerance += 1;
                    let _ = writeln!(table, "  {k}\n      new key, now {now}");
                    kept.insert(k.clone(), now.clone());
                }
            }
        }
        for (k, g) in &old {
            if !self.values.contains_key(k) {
                out_of_tolerance += 1;
                let _ = writeln!(table, "  {k}\n      removed key, golden {g}");
            }
        }
        if out_of_tolerance == 0 {
            return;
        }
        let header = format!(
            "golden {}: {out_of_tolerance} of {} values differ",
            path.display(),
            self.values.len()
        );
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Values within tolerance keep their text (a parse and print of a float may move
            // its last digit): only the values out of tolerance change in the diff.
            let raw = entries(&old_text);
            let lines: Vec<String> = kept
                .iter()
                .map(|(k, v)| match raw.get(k) {
                    Some(r) if unchanged.contains(k) => r.clone(),
                    _ => format!(
                        "  {}: {}",
                        Value::String(k.clone()),
                        serde_json::to_string_pretty(v)
                            .unwrap()
                            .replace('\n', "\n  ")
                    ),
                })
                .collect();
            let body = format!("{{\n{}\n}}\n", lines.join(",\n"));
            assert_eq!(
                serde_json::from_str::<BTreeMap<String, Value>>(&body)
                    .unwrap()
                    .len(),
                kept.len()
            );
            std::fs::write(&path, body).unwrap();
            println!("{header} (SKIMAP_BLESS: reference rewritten)\n{table}");
        } else {
            panic!(
                "{header}\n{table}To accept them: SKIMAP_BLESS=1 cargo test (same arguments), then review `git diff fixtures/golden/`."
            );
        }
    }
}

/// Top-level entries of a reference written by [`Golden::check`] (pretty JSON, two-space
/// indent): key → its text, without the separating comma.
fn entries(text: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut cur: Option<(String, String)> = None;
    for line in text.lines() {
        let top = line.starts_with("  \"") && !line.starts_with("   ");
        if top {
            if let Some((k, v)) = cur.take() {
                out.insert(k, v);
            }
            let end = line[3..].find("\": ").map(|i| i + 4);
            let key = end.and_then(|e| serde_json::from_str::<String>(&line[2..e]).ok());
            if let Some(k) = key {
                cur = Some((k, line.to_owned()));
                continue;
            }
        }
        if let Some((_, v)) = cur.as_mut()
            && line != "}"
        {
            v.push('\n');
            v.push_str(line);
        }
    }
    if let Some((k, v)) = cur {
        out.insert(k, v);
    }
    out.into_iter()
        .map(|(k, v)| (k, v.trim_end_matches(',').to_owned()))
        .collect()
}
