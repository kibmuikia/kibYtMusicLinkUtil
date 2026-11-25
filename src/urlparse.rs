//! Part-1..part-4 decomposition: `scheme://host[:port]/path?query#fragment`.
//!
//! Hand-rolled on purpose: the component model here IS the product surface
//! (segments are inspected, reported and policy-matched individually), so we
//! keep full control over tokenisation instead of adapting a general parser.
//! This is NOT a general-purpose URL parser — it targets YouTube-family links
//! and reports anything odd it meets as a `problem` rather than guessing.

use crate::json::Json;
use crate::videoid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputShape {
    Url,
    BareVideoId,
    #[default]
    Unknown,
}

impl InputShape {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputShape::Url => "url",
            InputShape::BareVideoId => "bare-video-id",
            InputShape::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueryPair {
    pub key_raw: String,
    pub key: String,
    pub value_raw: String,
    pub value: String,
    pub decoded_ok: bool,
}

impl QueryPair {
    pub fn to_json(&self) -> Json {
        let mut pairs: Vec<(&str, Json)> = vec![
            ("key", Json::str(self.key.clone())),
            ("value", Json::str(self.value.clone())),
            ("valueLength", Json::int(self.value.chars().count() as i64)),
            ("percentDecoded", Json::Bool(self.key_raw != self.key || self.value_raw != self.value)),
        ];
        if !self.decoded_ok {
            pairs.push(("decodeProblem", Json::Bool(true)));
        }
        Json::obj(pairs)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ParsedInput {
    pub raw: String,
    pub shape: InputShape,
    pub scheme: Option<String>,
    pub userinfo: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
    pub path: Option<String>,
    pub query: Vec<QueryPair>,
    pub fragment: Option<String>,
    pub problems: Vec<String>,
}

pub fn has_percent_encoding(s: &str) -> bool {
    s.contains('%')
}

/// Percent-decoding for query components. `+` is decoded as space only when
/// `plus_as_space` is set (classic application/x-www-form-urlencoded rule).
/// Returns `(decoded, ok)` — `ok == false` means a malformed escape was kept
/// verbatim so the caller can flag it instead of silently corrupting the value.
pub fn percent_decode(s: &str, plus_as_space: bool) -> (String, bool) {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut ok = true;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 3;
                }
                _ => {
                    ok = false;
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b'%' => {
                ok = false;
                out.push(bytes[i]);
                i += 1;
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), ok)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Strip surrounding whitespace plus one layer of wrapping quotes/angle
/// brackets — the shapes you get when a link is pasted out of chat or markdown.
fn clean(s: &str) -> &str {
    let mut t = s.trim();
    for (open, close) in [('"', '"'), ('\'', '\''), ('<', '>')] {
        if t.len() >= 2 && t.starts_with(open) && t.ends_with(close) {
            t = t[1..t.len() - 1].trim();
        }
    }
    t
}

pub fn parse(input: &str) -> ParsedInput {
    let mut out = ParsedInput {
        raw: input.to_string(),
        ..Default::default()
    };
    let trimmed = clean(&input);
    if trimmed.is_empty() {
        out.problems.push("empty input".to_string());
        return out;
    }

    // ---- scheme -------------------------------------------------------
    let mut rest: &str;
    if let Some(idx) = trimmed.find("://") {
        let sch = &trimmed[..idx];
        if sch.is_empty()
            || !sch
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        {
            out.problems.push(format!("malformed scheme '{sch}'"));
        }
        out.scheme = Some(sch.to_ascii_lowercase());
        rest = &trimmed[idx + 3..];
    } else if let Some(stripped) = trimmed.strip_prefix("//") {
        out.problems.push("protocol-relative input: scheme is missing".to_string());
        rest = stripped;
    } else {
        // No scheme at all. Either a bare video id, or a scheme-less URL.
        if trimmed.len() == videoid::CANONICAL_LEN && videoid::is_charset_ok(trimmed) {
            out.shape = InputShape::BareVideoId;
            out.problems
                .push("input is a bare videoId, not a URL (no scheme/host/path to validate)".to_string());
            return out;
        }
        out.problems.push(
            "no 'scheme://' found; parsing the remainder as host/path in case this is a scheme-less URL"
                .to_string(),
        );
        rest = trimmed;
    }

    // ---- authority (host[:port]) -------------------------------------
    let auth_end = rest.find(is_component_sep).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    rest = &rest[auth_end..];

    if authority.is_empty() {
        if out.scheme.is_some() {
            out.problems.push("missing authority (host)".to_string());
        }
    } else {
        let mut hostport = authority;
        if let Some(at) = authority.rfind('@') {
            out.userinfo = Some(authority[..at].to_string());
            out.problems
                .push("userinfo (user:pass@) present; never expected on YouTube links".to_string());
            hostport = &authority[at + 1..];
        }

        if hostport.starts_with('[') {
            match hostport.find(']') {
                Some(close) => {
                    out.host = Some(hostport[..close + 1].to_ascii_lowercase());
                    if let Some(port) = hostport[close + 1..].strip_prefix(':') {
                        out.port = Some(port.to_string());
                    }
                }
                None => out.problems.push("unterminated IPv6 host literal".to_string()),
            }
        } else if let Some(colon) = hostport.rfind(':') {
            let (h, p) = (&hostport[..colon], &hostport[colon + 1..]);
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                out.host = Some(h.to_ascii_lowercase());
                out.port = Some(p.to_string());
            } else {
                out.host = Some(hostport.to_ascii_lowercase());
                out.problems
                    .push("host contains ':' but the suffix is not a port".to_string());
            }
        } else {
            out.host = Some(hostport.to_ascii_lowercase());
        }

        if let Some(h) = &out.host {
            if h.ends_with('.') {
                out.problems.push("host has a trailing dot".to_string());
            }
            if h.contains(' ') || h.contains('_') {
                out.problems
                    .push("host contains characters that are invalid in a DNS name".to_string());
            }
            if h.is_empty() {
                out.problems.push("host is empty".to_string());
            }
        }
    }

    // ---- path / query / fragment --------------------------------------
    let (path_str, tail) = match rest.find(is_query_or_fragment) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if !path_str.is_empty() {
        if !path_str.starts_with('/') {
            out.problems
                .push(format!("path does not start with '/': '{path_str}'"));
        }
        out.path = Some(path_str.to_string());
    }

    if let Some(q) = tail.strip_prefix('?') {
        let (qs, frag) = match q.find('#') {
            Some(i) => (&q[..i], Some(&q[i + 1..])),
            None => (q, None),
        };
        out.fragment = frag.map(|f| f.to_string());
        out.query = parse_query(qs);
        if !qs.is_empty() && out.query.is_empty() {
            out.problems
                .push("query string present but no key/value pair parsed".to_string());
        }
    } else if let Some(f) = tail.strip_prefix('#') {
        out.fragment = Some(f.to_string());
        out.problems
            .push("fragment present but no query string; a 'v' param cannot live in a fragment".to_string());
    }

    if out.shape == InputShape::Unknown {
        out.shape = match &out.host {
            Some(h) if !h.is_empty() && !h.contains(' ') && !h.contains('_') => InputShape::Url,
            _ => InputShape::Unknown,
        };
    }

    out
}

fn is_component_sep(c: char) -> bool {
    matches!(c, '/' | '?' | '#')
}

fn is_query_or_fragment(c: char) -> bool {
    matches!(c, '?' | '#')
}

fn parse_query(q: &str) -> Vec<QueryPair> {
    let mut pairs = Vec::new();
    for part in q.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k_raw, v_raw) = match part.split_once('=') {
            Some((k, v)) => (k, v),
            None => (part, ""),
        };
        let (k, k_ok) = percent_decode(k_raw, true);
        let (v, v_ok) = percent_decode(v_raw, true);
        pairs.push(QueryPair {
            key_raw: k_raw.to_string(),
            key: k,
            value_raw: v_raw.to_string(),
            value: v,
            decoded_ok: k_ok && v_ok,
        });
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposes_the_two_sample_links() {
        for (url, id) in [
            ("https://music.youtube.com/watch?v=qARrn7G067w", "qARrn7G067w"),
            ("https://music.youtube.com/watch?v=AHsnSK0quvc", "AHsnSK0quvc"),
        ] {
            let p = parse(url);
            assert_eq!(p.shape, InputShape::Url);
            assert_eq!(p.scheme.as_deref(), Some("https"));
            assert_eq!(p.host.as_deref(), Some("music.youtube.com"));
            assert_eq!(p.path.as_deref(), Some("/watch"));
            assert_eq!(p.query.len(), 1);
            assert_eq!(p.query[0].key, "v");
            assert_eq!(p.query[0].value, id);
            assert!(p.problems.is_empty(), "{:?}", p.problems);
        }
    }

    #[test]
    fn strips_wrapping_and_keeps_tail_intact() {
        let p = parse("<https://music.youtube.com/watch?v=qARrn7G067w&list=RD&si=x> ");
        assert_eq!(p.host.as_deref(), Some("music.youtube.com"));
        assert_eq!(p.query.len(), 3);
        assert_eq!(p.query[0].value, "qARrn7G067w");
    }

    #[test]
    fn detects_bare_id_and_scheme_less_url() {
        assert_eq!(parse("qARrn7G067w").shape, InputShape::BareVideoId);
        let p = parse("music.youtube.com/watch?v=qARrn7G067w");
        assert_eq!(p.scheme, None);
        assert_eq!(p.host.as_deref(), Some("music.youtube.com"));
        assert_eq!(p.path.as_deref(), Some("/watch"));
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b", true).0, "a b");
        assert_eq!(percent_decode("a+b", true).0, "a b");
        assert_eq!(percent_decode("a+b", false).0, "a+b");
        assert_eq!(percent_decode("a%zz", true).1, false);
        assert_eq!(percent_decode("%3D", true).0, "=");
    }
}
