//! Minimal dependency-free JSON writer.
//!
//! We emit JSON ourselves instead of pulling serde/serde_json so the crate
//! builds with zero network access and zero supply-chain surface. Only the
//! writer exists; we never need to parse JSON in phase 1 (the spec file uses
//! its own line DSL, see `spec.rs`).

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn str<S: Into<String>>(s: S) -> Json {
        Json::Str(s.into())
    }

    pub fn opt_str<S: Into<String>>(s: Option<S>) -> Json {
        match s {
            Some(v) => Json::Str(v.into()),
            None => Json::Null,
        }
    }

    pub fn int<I: Into<i64>>(i: I) -> Json {
        Json::Int(i.into())
    }

    pub fn arr<I: IntoIterator<Item = Json>>(items: I) -> Json {
        Json::Arr(items.into_iter().collect())
    }

    /// `strings(&["a","b"])` -> `["a","b"]`
    pub fn strings<I, S>(items: I) -> Json
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Json::arr(items.into_iter().map(|s| Json::Str(s.into())))
    }

    pub fn obj<I, K>(pairs: I) -> Json
    where
        I: IntoIterator<Item = (K, Json)>,
        K: Into<String>,
    {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_pretty(&self, out: &mut String, depth: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(i) => {
                let _ = write!(out, "{}", i);
            }
            Json::Float(f) => {
                if f.is_finite() {
                    let _ = write!(out, "{}", f);
                } else {
                    out.push_str("null"); // JSON has no NaN/Inf
                }
            }
            Json::Str(s) => write_escaped(out, s),
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    pad(out, depth + 1);
                    item.write_pretty(out, depth + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, depth);
                out.push(']');
            }
            Json::Obj(pairs) => {
                if pairs.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (k, v)) in pairs.iter().enumerate() {
                    pad(out, depth + 1);
                    write_escaped(out, k);
                    out.push_str(": ");
                    v.write_pretty(out, depth + 1);
                    if i + 1 < pairs.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, depth);
                out.push('}');
            }
        }
    }
}

fn pad(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_escaped(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_control_and_quotes() {
        let j = Json::obj(vec![("k", Json::str("a\"b\\c\nd"))]);
        let s = j.to_pretty();
        assert!(s.contains("\\\""), "{}", s);
        assert!(s.contains("\\\\"), "{}", s);
        assert!(s.contains("\\n"), "{}", s);
    }

    #[test]
    fn empty_shapes() {
        assert_eq!(Json::arr(Vec::new()).to_pretty(), "[]");
        assert_eq!(Json::obj(Vec::<(String, Json)>::new()).to_pretty(), "{}");
    }
}
