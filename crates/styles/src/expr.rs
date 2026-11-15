//! The subset of MapLibre expressions the styles may use, and an evaluator of filters
//! on real features (to prove that no layer is orphan).
//!
//! The subset is supported alike by MapLibre GL JS (viewer) and MapLibre Native (app):
//! no legacy filter syntax, no recent operators.

use serde_json::Value;

/// Operators allowed in the styles.
pub const OPERATORS: &[&str] = &[
    // Lookup and types.
    "get",
    "has",
    "literal",
    "geometry-type",
    "zoom",
    "to-string",
    "to-number",
    // Paint only: state set by the app (`metadata.skimap:feature_states`).
    "feature-state",
    // Decisions.
    "match",
    "case",
    "coalesce",
    "==",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "all",
    "any",
    "!",
    "in",
    // Ramps and maths.
    "interpolate",
    "step",
    "linear",
    "exponential",
    "*",
    "+",
    "-",
    "/",
];

/// Layout and paint properties whose values are plain arrays, not expressions.
pub const LITERAL_ARRAYS: &[&str] = &[
    "text-font",
    "text-offset",
    "line-dasharray",
    "icon-offset",
    "text-variable-anchor",
    "line-translate",
    "fill-translate",
    "circle-translate",
    "text-translate",
    "icon-translate",
    "icon-text-fit-padding",
];

/// Calls `f` for every sub-expression of `v` (an expression, not a literal array).
pub fn walk<'a>(v: &'a Value, f: &mut dyn FnMut(&'a [Value])) {
    let Value::Array(a) = v else { return };
    let Some(op) = a.first().and_then(Value::as_str) else {
        return;
    };
    f(a);
    match op {
        "literal" => {}
        "match" => {
            // ["match", input, labels, output, ..., default]: labels are literals.
            if let Some(input) = a.get(1) {
                walk(input, f);
            }
            let rest = &a[2.min(a.len())..];
            for (i, x) in rest.iter().enumerate() {
                let is_label = i % 2 == 0 && i + 1 < rest.len();
                if !is_label {
                    walk(x, f);
                }
            }
        }
        "interpolate" => {
            // ["interpolate", ["linear"], input, stop, output, ...]
            for x in a.iter().skip(2) {
                walk(x, f);
            }
        }
        _ => {
            for x in &a[1..] {
                walk(x, f);
            }
        }
    }
}

/// A feature as seen by a filter.
pub trait Feature {
    /// Property value (`None` if absent).
    fn get(&self, key: &str) -> Option<Value>;
    /// MapLibre geometry type (`Point`, `LineString`, `Polygon`).
    fn geometry_type(&self) -> &str;
}

/// Evaluation error: an operator outside [`OPERATORS`] or a malformed expression.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct EvalError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError(msg.into()))
}

/// Evaluates an expression on a feature at a zoom.
pub fn eval(e: &Value, f: &dyn Feature, zoom: f64) -> Result<Value, EvalError> {
    let Value::Array(a) = e else {
        return Ok(e.clone());
    };
    let Some(op) = a.first().and_then(Value::as_str) else {
        return err(format!("not an expression: {e}"));
    };
    let arg = |i: usize| -> Result<Value, EvalError> {
        a.get(i)
            .ok_or_else(|| EvalError(format!("{op}: missing argument {i}")))
            .and_then(|x| eval(x, f, zoom))
    };
    Ok(match op {
        "literal" => a.get(1).cloned().unwrap_or(Value::Null),
        "get" => {
            let k = arg(1)?;
            f.get(k.as_str().unwrap_or_default()).unwrap_or(Value::Null)
        }
        "has" => {
            let k = arg(1)?;
            Value::Bool(
                f.get(k.as_str().unwrap_or_default())
                    .is_some_and(|v| !v.is_null()),
            )
        }
        "geometry-type" => Value::String(f.geometry_type().to_string()),
        "zoom" => serde_json::json!(zoom),
        "to-string" => match arg(1)? {
            Value::String(s) => Value::String(s),
            Value::Null => Value::String(String::new()),
            v => Value::String(v.to_string()),
        },
        "to-number" => match arg(1)? {
            Value::Number(n) => Value::Number(n),
            Value::String(s) => s
                .parse::<f64>()
                .map(|x| serde_json::json!(x))
                .unwrap_or(Value::Null),
            Value::Bool(b) => serde_json::json!(if b { 1 } else { 0 }),
            _ => serde_json::json!(0),
        },
        "coalesce" => {
            for i in 1..a.len() {
                let v = arg(i)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Value::Null
        }
        "==" | "!=" => {
            let same = values_equal(&arg(1)?, &arg(2)?);
            Value::Bool(if op == "==" { same } else { !same })
        }
        "<" | "<=" | ">" | ">=" => {
            let (x, y) = (arg(1)?, arg(2)?);
            let ord = match (&x, &y) {
                (Value::Number(p), Value::Number(q)) => p
                    .as_f64()
                    .zip(q.as_f64())
                    .and_then(|(p, q)| p.partial_cmp(&q)),
                (Value::String(p), Value::String(q)) => Some(p.cmp(q)),
                _ => return err(format!("{op}: incomparable {x} and {y}")),
            };
            let Some(o) = ord else {
                return Ok(Value::Bool(false));
            };
            Value::Bool(match op {
                "<" => o.is_lt(),
                "<=" => o.is_le(),
                ">" => o.is_gt(),
                _ => o.is_ge(),
            })
        }
        "all" => {
            for i in 1..a.len() {
                if !truthy(&arg(i)?) {
                    return Ok(Value::Bool(false));
                }
            }
            Value::Bool(true)
        }
        "any" => {
            for i in 1..a.len() {
                if truthy(&arg(i)?) {
                    return Ok(Value::Bool(true));
                }
            }
            Value::Bool(false)
        }
        "!" => Value::Bool(!truthy(&arg(1)?)),
        "in" => {
            let (needle, hay) = (arg(1)?, arg(2)?);
            Value::Bool(match (&needle, &hay) {
                (_, Value::Array(items)) => items.iter().any(|x| values_equal(x, &needle)),
                (Value::String(n), Value::String(h)) => h.contains(n.as_str()),
                (_, Value::Null) => false,
                _ => return err(format!("in: bad haystack {hay}")),
            })
        }
        "match" => {
            let input = arg(1)?;
            let rest = &a[2..];
            if rest.len() < 3 || rest.len() % 2 == 0 {
                return err("match: wrong number of arguments");
            }
            for pair in rest[..rest.len() - 1].chunks(2) {
                let hit = match &pair[0] {
                    Value::Array(labels) => labels.iter().any(|l| values_equal(l, &input)),
                    l => values_equal(l, &input),
                };
                if hit {
                    return eval(&pair[1], f, zoom);
                }
            }
            eval(&rest[rest.len() - 1], f, zoom)?
        }
        "case" => {
            let rest = &a[1..];
            if rest.len() < 3 || rest.len() % 2 == 0 {
                return err("case: wrong number of arguments");
            }
            for pair in rest[..rest.len() - 1].chunks(2) {
                if truthy(&eval(&pair[0], f, zoom)?) {
                    return eval(&pair[1], f, zoom);
                }
            }
            eval(&rest[rest.len() - 1], f, zoom)?
        }
        "step" => {
            let input = arg(1)?.as_f64().unwrap_or(0.0);
            let mut out = arg(2)?;
            for pair in a[3..].chunks(2) {
                if pair.len() == 2 && pair[0].as_f64().is_some_and(|s| input >= s) {
                    out = eval(&pair[1], f, zoom)?;
                }
            }
            out
        }
        "interpolate" => {
            let input = arg(2)?.as_f64().unwrap_or(0.0);
            let stops: Vec<(f64, Value)> = a[3..]
                .chunks(2)
                .filter(|p| p.len() == 2)
                .map(|p| Ok((p[0].as_f64().unwrap_or(0.0), eval(&p[1], f, zoom)?)))
                .collect::<Result<_, EvalError>>()?;
            let num = |v: &Value| v.as_f64().unwrap_or(0.0);
            match stops.as_slice() {
                [] => Value::Null,
                [(_, v)] => v.clone(),
                _ => {
                    let (z0, v0) = &stops[0];
                    let (zn, vn) = &stops[stops.len() - 1];
                    if input <= *z0 {
                        v0.clone()
                    } else if input >= *zn {
                        vn.clone()
                    } else {
                        let i = stops.iter().position(|(z, _)| *z > input).unwrap_or(1);
                        let ((za, va), (zb, vb)) = (&stops[i - 1], &stops[i]);
                        let t = (input - za) / (zb - za);
                        serde_json::json!(num(va) + t * (num(vb) - num(va)))
                    }
                }
            }
        }
        "*" | "+" | "-" | "/" => {
            let xs: Vec<f64> = (1..a.len())
                .map(|i| arg(i).map(|v| v.as_f64().unwrap_or(0.0)))
                .collect::<Result<_, _>>()?;
            let r = match op {
                "*" => xs.iter().product(),
                "+" => xs.iter().sum(),
                "-" if xs.len() == 1 => -xs[0],
                "-" => xs[0] - xs[1..].iter().sum::<f64>(),
                _ => xs[0] / xs.get(1).copied().unwrap_or(1.0),
            };
            serde_json::json!(r)
        }
        other => return err(format!("operator {other:?} outside the supported subset")),
    })
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// Truth of a filter result (MapLibre: only `true` passes).
pub fn truthy(v: &Value) -> bool {
    v == &Value::Bool(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct F(Value, &'static str);
    impl Feature for F {
        fn get(&self, key: &str) -> Option<Value> {
            self.0.get(key).cloned()
        }
        fn geometry_type(&self) -> &str {
            self.1
        }
    }

    #[test]
    fn evaluates_the_filters_of_the_styles() {
        let f = F(
            json!({"uses": ["nordic", "hike"], "difficulty": "easy", "exposure": 3, "rules_text": "Bivouac : interdit (toute l'année)."}),
            "LineString",
        );
        let t = |e: Value| truthy(&eval(&e, &f, 14.0).unwrap());
        assert!(t(json!([
            "in",
            "nordic",
            ["coalesce", ["get", "uses"], ["literal", []]]
        ])));
        assert!(!t(json!(["in", "skating", ["get", "uses"]])));
        assert!(t(json!([
            "in",
            "Bivouac : interdit",
            ["coalesce", ["get", "rules_text"], ""]
        ])));
        assert!(t(json!([
            "match",
            ["get", "difficulty"],
            ["easy", "novice"],
            true,
            false
        ])));
        assert!(t(json!([">=", ["coalesce", ["get", "exposure"], 0], 3])));
        assert!(!t(json!(["has", "name"])));
        assert!(t(json!(["==", ["geometry-type"], "LineString"])));
        assert_eq!(
            eval(
                &json!(["interpolate", ["linear"], ["zoom"], 10, 1, 16, 4]),
                &f,
                12.0
            )
            .unwrap(),
            json!(2.0)
        );
        assert_eq!(
            eval(&json!(["step", ["zoom"], 0, 13, 12.5]), &f, 14.0).unwrap(),
            json!(12.5)
        );
        assert!(eval(&json!(["within", {}]), &f, 14.0).is_err());
    }

    #[test]
    fn walks_match_outputs_but_not_labels() {
        let e = json!([
            "match",
            ["get", "kind"],
            ["a", "b"],
            ["get", "x"],
            "c",
            1,
            ["get", "y"]
        ]);
        let mut gets = Vec::new();
        walk(&e, &mut |a| {
            if a[0] == "get" {
                gets.push(a[1].clone());
            }
        });
        assert_eq!(gets, vec![json!("kind"), json!("x"), json!("y")]);
    }
}
