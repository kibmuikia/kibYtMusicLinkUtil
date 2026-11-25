//! Part-4: offline validation of the `v` (videoId) value.
//!
//! Every check is named, classified and reported — the report is the audit
//! trail, so "what did you validate?" is answerable from the JSON alone.

use crate::json::Json;
use crate::urlparse;

/// URL-safe Base64 alphabet, in index order. A YouTube id is a 64-bit integer
/// rendered in this alphabet to 11 characters.
pub const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
pub const CANONICAL_LEN: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntropyMode {
    /// Do not evaluate the last-character rule at all.
    Off,
    /// Evaluate, but only warn (default: the rule holds for all observed ids
    /// but is not a documented contract).
    Report,
    /// Evaluate and treat a failure as an error.
    Strict,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub code: &'static str,
    pub category: &'static str,
    pub rule: String,
    pub passed: bool,
    pub severity: Severity,
    pub detail: String,
}

impl Check {
    fn to_json(&self) -> Json {
        Json::obj(vec![
            ("code", Json::str(self.code)),
            ("category", Json::str(self.category)),
            ("rule", Json::str(self.rule.clone())),
            ("severity", Json::str(self.severity.as_str())),
            ("passed", Json::Bool(self.passed)),
            ("detail", Json::str(self.detail.clone())),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct VideoIdValidation {
    pub candidate: String,
    pub valid: bool,
    pub length: usize,
    pub mode: EntropyMode,
    pub checks: Vec<Check>,
}

pub fn is_charset_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| ALPHABET.contains(c))
}

/// A 64-bit id in 11 base64 chars = 66 bits of alphabet, so the final char
/// only carries 4 significant bits: its alphabet index must be a multiple
/// of 4 (16 of 64 characters are possible). Cheap, strong typo/splice catcher.
pub fn last_char_entropy_ok(s: &str) -> bool {
    match s.chars().last() {
        Some(c) => ALPHABET.find(c).map(|i| i % 4 == 0).unwrap_or(false),
        None => false,
    }
}

pub fn validate(candidate: &str, mode: EntropyMode) -> VideoIdValidation {
    let mut checks: Vec<Check> = Vec::new();
    let len = candidate.chars().count();

    checks.push(Check {
        code: "VID_NOT_EMPTY",
        category: "structural",
        rule: "value is non-empty".to_string(),
        passed: !candidate.is_empty(),
        severity: Severity::Error,
        detail: format!("{} byte(s)", candidate.len()),
    });

    checks.push(Check {
        code: "VID_LENGTH_EQ_11",
        category: "length",
        rule: format!("exactly {CANONICAL_LEN} characters (canonical length)"),
        passed: len == CANONICAL_LEN,
        severity: Severity::Error,
        detail: format!("observed {len}"),
    });

    let charset_ok = is_charset_ok(candidate);
    let offenders: String = candidate.chars().filter(|c| !ALPHABET.contains(*c)).collect();
    checks.push(Check {
        code: "VID_CHARSET_URLSAFE_BASE64",
        category: "charset",
        rule: "characters drawn from [A-Za-z0-9_-]".to_string(),
        passed: charset_ok,
        severity: Severity::Error,
        detail: if charset_ok {
            "all characters inside the alphabet".to_string()
        } else {
            format!("offending characters: {offenders:?}")
        },
    });

    let has_percent = urlparse::has_percent_encoding(candidate);
    checks.push(Check {
        code: "VID_NO_PERCENT_ENCODING",
        category: "encoding",
        rule: "no percent-encoding inside the id".to_string(),
        passed: !has_percent,
        severity: Severity::Error,
        detail: if has_percent {
            "found '%' — the parameter is double-encoded, truncated, or the '&' was escaped".to_string()
        } else {
            "no '%' present".to_string()
        },
    });

    let has_padding = candidate.contains('=') || candidate.contains('+') || candidate.contains('/');
    checks.push(Check {
        code: "VID_NO_BASE64_PADDING",
        category: "encoding",
        rule: "unpadded URL-safe alphabet ('=' '+' '/' are invalid)".to_string(),
        passed: !has_padding,
        severity: Severity::Error,
        detail: if has_padding {
            "standard-alphabet or padded base64 leaked into the value".to_string()
        } else {
            "no padding or standard-alphabet characters".to_string()
        },
    });

    let has_ws = candidate
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>');
    checks.push(Check {
        code: "VID_NO_WHITESPACE_OR_QUOTES",
        category: "structural",
        rule: "no whitespace, quotes or angle brackets".to_string(),
        passed: !has_ws,
        severity: Severity::Error,
        detail: if has_ws {
            "input carried a stray separator/quoting character".to_string()
        } else {
            "clean".to_string()
        },
    });

    match mode {
        EntropyMode::Off => checks.push(Check {
            code: "VID_LAST_CHAR_ENTROPY",
            category: "entropy",
            rule: "last character index is a multiple of 4".to_string(),
            passed: true,
            severity: Severity::Info,
            detail: "check disabled by configuration".to_string(),
        }),
        _ => {
            let ok = last_char_entropy_ok(candidate);
            let detail = match candidate.chars().last() {
                Some(c) => match ALPHABET.find(c) {
                    Some(i) => format!(
                        "last char '{c}' has alphabet index {i}; mod-4 residue {} (only residue 0 is legal)",
                        i % 4
                    ),
                    None => format!("last char '{c}' is not in the alphabet"),
                },
                None => "no characters".to_string(),
            };
            checks.push(Check {
                code: "VID_LAST_CHAR_ENTROPY",
                category: "entropy",
                rule: "last character index is a multiple of 4 (16 of 64 values legal)".to_string(),
                passed: ok,
                severity: if mode == EntropyMode::Strict {
                    Severity::Error
                } else {
                    Severity::Warn
                },
                detail,
            });
        }
    }

    checks.push(Check {
        code: "VID_CASE_SENSITIVE",
        category: "semantics",
        rule: "ids are case-sensitive; never lowercase them".to_string(),
        passed: true,
        severity: Severity::Info,
        detail: "informational: 'aB3' and 'Ab3' are different ids".to_string(),
    });

    let valid = checks
        .iter()
        .all(|c| c.passed || c.severity != Severity::Error);

    VideoIdValidation {
        candidate: candidate.to_string(),
        valid,
        length: len,
        mode,
        checks,
    }
}

impl VideoIdValidation {
    pub fn failed_errors(&self) -> Vec<&Check> {
        self.checks
            .iter()
            .filter(|c| !c.passed && c.severity == Severity::Error)
            .collect()
    }

    pub fn has_soft_failure(&self) -> bool {
        self.checks
            .iter()
            .any(|c| !c.passed && c.severity == Severity::Warn)
    }

    pub fn validation_types(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in &self.checks {
            let t = c.category.to_string();
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out
    }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![
            ("value", Json::str(self.candidate.clone())),
            ("valid", Json::Bool(self.valid)),
            ("length", Json::int(self.length as i64)),
            ("canonicalLength", Json::int(CANONICAL_LEN as i64)),
            ("alphabet", Json::str("url-safe base64 (A-Z a-z 0-9 - _), unpadded")),
            ("entropyMode", Json::str(match self.mode {
                EntropyMode::Off => "off",
                EntropyMode::Report => "report",
                EntropyMode::Strict => "strict",
            })),
            ("validationTypes", Json::strings(self.validation_types())),
            ("checksApplied", Json::int(self.checks.len() as i64)),
            (
                "checks",
                Json::arr(self.checks.iter().map(|c| c.to_json())),
            ),
            (
                "onlineConfirmation",
                Json::str("deferred: phase 1 performs no network requests"),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_ids_pass() {
        for id in ["qARrn7G067w", "AHsnSK0quvc", "dQw4w9WgXcQ"] {
            let v = validate(id, EntropyMode::Strict);
            assert!(v.valid, "expected valid: {} -> {:?}", id, v.failed_errors());
        }
    }

    #[test]
    fn entropy_rule_rejects_wrong_last_char() {
        assert!(last_char_entropy_ok("qARrn7G067w")); // 'w' -> 48
        assert!(last_char_entropy_ok("AHsnSK0quvc")); // 'c' -> 28
        assert!(!last_char_entropy_ok("qARrn7G067x")); // 'x' -> 49
        assert!(!last_char_entropy_ok(""));
    }

    #[test]
    fn hard_failures() {
        assert!(!validate("qARrn7G067", EntropyMode::Report).valid); // 10 chars
        assert!(!validate("qARrn7G067ww", EntropyMode::Report).valid); // 12 chars
        assert!(!validate("qARr%7G067w", EntropyMode::Report).valid); // percent
        assert!(!validate("qARrn7G067/", EntropyMode::Report).valid); // standard-alphabet '/'
        assert!(!validate("qARrn7 G067w", EntropyMode::Report).valid); // whitespace
    }

    #[test]
    fn entropy_soft_vs_strict() {
        let soft = validate("qARrn7G067x", EntropyMode::Report);
        assert!(soft.valid, "soft mode must not hard-fail");
        assert!(soft.has_soft_failure());
        let strict = validate("qARrn7G067x", EntropyMode::Strict);
        assert!(!strict.valid);
    }
}
