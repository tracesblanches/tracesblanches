//! Change journal shared by the refreshes of the refuges and stations layers (H1, H2):
//! compare two versions of a set of records, keep the date each record last changed,
//! append the changes to `journal.jsonl`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

/// A change between two runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    /// Date of the run.
    pub at: String,
    /// `stations`, `titres`, `sources`.
    pub part: String,
    /// Id of the record (domain, title, source).
    pub id: String,
    /// `added`, `removed`, `changed`, or a source status (`unreachable`, `broken`,
    /// `changed`, `ok`).
    pub change: String,
    /// Changed top-level fields, or the error.
    pub detail: Vec<String>,
}

/// Keys ignored when comparing two versions of a record (dates of the check itself).
pub const VOLATILE: &[&str] = &[
    "checked_at",
    "changed_at",
    "fetched_at",
    "status",
    "age_days",
];

fn strip(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !VOLATILE.contains(&k.as_str()))
                .map(|(k, x)| (k.clone(), strip(x)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(strip).collect()),
        x => x.clone(),
    }
}

/// Changes between two sets of records keyed by id.
pub fn diff(
    old: &BTreeMap<String, Value>,
    new: &BTreeMap<String, Value>,
    part: &str,
    at: &str,
) -> Vec<Change> {
    let mut out = Vec::new();
    let mk = |id: &str, change: &str, detail: Vec<String>| Change {
        at: at.to_owned(),
        part: part.to_owned(),
        id: id.to_owned(),
        change: change.to_owned(),
        detail,
    };
    for (id, n) in new {
        match old.get(id) {
            None => out.push(mk(id, "added", Vec::new())),
            Some(o) => {
                let (o, n) = (strip(o), strip(n));
                if o != n {
                    let keys: Vec<String> = match (&o, &n) {
                        (Value::Object(a), Value::Object(b)) => {
                            let mut k: Vec<String> = a
                                .keys()
                                .chain(b.keys())
                                .filter(|k| a.get(*k) != b.get(*k))
                                .cloned()
                                .collect();
                            k.sort();
                            k.dedup();
                            k
                        }
                        _ => Vec::new(),
                    };
                    out.push(mk(id, "changed", keys));
                }
            }
        }
    }
    for id in old.keys().filter(|k| !new.contains_key(*k)) {
        out.push(mk(id, "removed", Vec::new()));
    }
    out
}

/// `changed_at` of each new record: kept from the old record when unchanged, else `at`.
pub fn carry_changed_at(
    old: &BTreeMap<String, Value>,
    new: &mut BTreeMap<String, Value>,
    at: &str,
) {
    for (id, n) in new.iter_mut() {
        let same = old.get(id).filter(|o| strip(o) == strip(n));
        let changed_at = same
            .and_then(|o| o.get("changed_at").filter(|v| !v.is_null()).cloned())
            .unwrap_or_else(|| json!(at));
        if let Value::Object(m) = n {
            m.insert("changed_at".into(), changed_at);
        }
    }
}

/// Appends changes to `journal.jsonl`.
pub fn append_journal(path: &Path, changes: &[Change]) -> std::io::Result<()> {
    use std::io::Write;
    if changes.is_empty() {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for c in changes {
        writeln!(f, "{}", serde_json::to_string(c).unwrap_or_default())?;
    }
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs() {
        let old: BTreeMap<String, Value> = [
            (
                "a".to_owned(),
                json!({"x": 1, "checked_at": "d1", "changed_at": "d0"}),
            ),
            ("b".to_owned(), json!({"x": 1})),
        ]
        .into();
        let mut new: BTreeMap<String, Value> = [
            ("a".to_owned(), json!({"x": 1, "checked_at": "d2"})),
            ("c".to_owned(), json!({"x": 2})),
        ]
        .into();
        let d = diff(&old, &new, "titres", "d2");
        let kinds: Vec<(&str, &str)> = d
            .iter()
            .map(|c| (c.id.as_str(), c.change.as_str()))
            .collect();
        assert_eq!(kinds, vec![("c", "added"), ("b", "removed")]);
        carry_changed_at(&old, &mut new, "d2");
        assert_eq!(new["a"]["changed_at"], "d0", "unchanged: date kept");
        assert_eq!(new["c"]["changed_at"], "d2");
        let changed: BTreeMap<String, Value> = [("a".to_owned(), json!({"x": 3}))].into();
        assert_eq!(diff(&old, &changed, "t", "d")[0].detail, vec!["x"]);
    }
}
