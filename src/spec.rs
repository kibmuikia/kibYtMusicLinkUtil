//! Segment spec: the data-driven description of "what a valid link looks like".
//!
//! Segments are a `Vec<SegmentRule>`, not hard-coded ifs, so a later phase can
//! add (e.g. `query:list`, `query:t`), remove (`part-2` shortener handling) or
//! retarget (`part-3` -> `/watch` | `/embed/<id>`) without touching the engine.
//! A spec file patches the built-in v1 spec at startup.

use std::path::Path;

use crate::json::Json;
use crate::version;
use crate::videoid::{EntropyMode, Severity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Scheme,
    Host,
    Path,
    QueryParam(String),
}

impl SegmentKind {
    pub fn label(&self) -> String {
        match self {
            SegmentKind::Scheme => "scheme".to_string(),
            SegmentKind::Host => "host".to_string(),
            SegmentKind::Path => "path".to_string(),
            SegmentKind::QueryParam(n) => format!("query.{n}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Matcher {
    /// value must equal one of these (host comparison is case-insensitive)
    Exact(Vec<String>),
    /// presence only
    Present,
    /// value is validated as a video id (`videoid::validate`)
    VideoId,
}

impl Matcher {
    pub fn describe(&self) -> String {
        match self {
            Matcher::Exact(v) => v.join(" | "),
            Matcher::Present => "<present>".to_string(),
            Matcher::VideoId => "<videoId>".to_string(),
        }
    }

    pub fn matches(&self, value: &str) -> bool {
        match self {
            Matcher::Exact(list) => list.iter().any(|e| e.eq_ignore_ascii_case(value)),
            Matcher::Present => true,
            Matcher::VideoId => true, // handled by the engine, not a literal match
        }
    }
}

#[derive(Clone, Debug)]
pub struct SegmentRule {
    pub id: String,
    pub name: String,
    pub kind: SegmentKind,
    pub matcher: Matcher,
    pub required: bool,
    pub severity: Severity,
    pub enabled: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct HostAlias {
    pub host: String,
    /// `same-video-namespace` | `shortener` | anything the spec author writes
    pub resolution: String,
    pub phase: u32,
    pub accepted: bool,
    pub code: String,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub spec_version: String,
    pub primary_host: String,
    pub host_aliases: Vec<HostAlias>,
    pub entropy: EntropyMode,
    pub segments: Vec<SegmentRule>,
    /// params considered structural, kept in the canonical form
    pub keep_query_params: Vec<String>,
    /// params known to be share residue and safe to drop
    pub drop_query_params: Vec<String>,
}

impl Spec {
    /// Built-in phase-1 policy.
    pub fn v1() -> Spec {
        Spec {
            spec_version: version::SPEC_VERSION.to_string(),
            primary_host: "music.youtube.com".to_string(),
            host_aliases: vec![
                HostAlias {
                    host: "www.youtube.com".to_string(),
                    resolution: "same-video-namespace".to_string(),
                    phase: 2,
                    accepted: false,
                    code: "E_HOST_NAMESPACE_DEFERRED".to_string(),
                },
                HostAlias {
                    host: "youtube.com".to_string(),
                    resolution: "same-video-namespace".to_string(),
                    phase: 2,
                    accepted: false,
                    code: "E_HOST_NAMESPACE_DEFERRED".to_string(),
                },
                HostAlias {
                    host: "m.youtube.com".to_string(),
                    resolution: "same-video-namespace".to_string(),
                    phase: 2,
                    accepted: false,
                    code: "E_HOST_NAMESPACE_DEFERRED".to_string(),
                },
                HostAlias {
                    host: "youtu.be".to_string(),
                    resolution: "shortener".to_string(),
                    phase: 2,
                    accepted: false,
                    code: "E_HOST_SHORTENER_DEFERRED".to_string(),
                },
            ],
            entropy: EntropyMode::Report,
            keep_query_params: vec!["v".to_string()],
            drop_query_params: vec![
                "si".to_string(),
                "list".to_string(),
                "index".to_string(),
                "pp".to_string(),
                "feature".to_string(),
                "ab_channel".to_string(),
                "t".to_string(),
                "start".to_string(),
            ],
            segments: vec![
                SegmentRule {
                    id: "part-1".to_string(),
                    name: "scheme".to_string(),
                    kind: SegmentKind::Scheme,
                    matcher: Matcher::Exact(vec!["https".to_string()]),
                    required: true,
                    severity: Severity::Error,
                    enabled: true,
                    notes: vec![
                        "YouTube 301-upgrades http -> https, but a stored link must already be https".to_string(),
                        "scheme comparison is case-insensitive".to_string(),
                    ],
                },
                SegmentRule {
                    id: "part-2".to_string(),
                    name: "host".to_string(),
                    kind: SegmentKind::Host,
                    matcher: Matcher::Exact(vec!["music.youtube.com".to_string()]),
                    required: true,
                    severity: Severity::Error,
                    enabled: true,
                    notes: vec![
                        "v1 lock: music host only".to_string(),
                        "www.youtube.com shares the same videoId namespace (same asset), deferred to phase 2".to_string(),
                        "host comparison is case-insensitive; the port is never part of the host segment".to_string(),
                    ],
                },
                SegmentRule {
                    id: "part-3".to_string(),
                    name: "path".to_string(),
                    kind: SegmentKind::Path,
                    matcher: Matcher::Exact(vec!["/watch".to_string()]),
                    required: true,
                    severity: Severity::Error,
                    enabled: true,
                    notes: vec![
                        "'/watch' is the canonical player route; '/embed/<id>' and '/v/<id>' are embed routes".to_string(),
                        "a trailing slash is rejected in v1".to_string(),
                    ],
                },
                SegmentRule {
                    id: "part-4".to_string(),
                    name: "query.parameter.v".to_string(),
                    kind: SegmentKind::QueryParam("v".to_string()),
                    matcher: Matcher::VideoId,
                    required: true,
                    severity: Severity::Error,
                    enabled: true,
                    notes: vec![
                        "`v` is the only content-identifying token in a watch URL".to_string(),
                        "11 chars over url-safe base64, 64-bit id, case-sensitive".to_string(),
                        "duplicates are reported and the first occurrence is used".to_string(),
                    ],
                },
            ],
        }
    }

    pub fn load_file(&mut self, path: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        for (n, line) in text.lines().enumerate() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            self.apply_line(l)
                .map_err(|e| format!("{}:{}: {}", path.display(), n + 1, e))?;
        }
        Ok(())
    }

    /// Line DSL, deliberately tiny. One directive per line, `#` comments.
    ///
    /// ```text
    /// primary-host music.youtube.com
    /// spec-version 1.1
    /// alias www.youtube.com same-video-namespace phase=2 accepted=false code=E_HOST_NAMESPACE_DEFERRED
    /// entropy strict=true
    /// keep-param v
    /// drop-param list
    /// segment disable part-3
    /// segment enable part-3
    /// segment remove part-4
    /// segment add part-5 query:list required=false severity=warning
    /// segment set part-3 exact=/watch
    /// ```
    pub fn apply_line(&mut self, line: &str) -> Result<(), String> {
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.is_empty() {
            return Ok(());
        }
        match t[0] {
            "primary-host" => {
                self.primary_host = arg(&t, 1, "primary-host <host>")?.to_ascii_lowercase();
            }
            "spec-version" => {
                self.spec_version = arg(&t, 1, "spec-version <value>")?.to_string();
            }
            "alias" => {
                let host = arg(&t, 1, "alias <host> [resolution] [phase=N] [accepted=bool] [code=..]")?
                    .to_ascii_lowercase();
                let mut resolution = String::new();
                let mut phase = 2u32;
                let mut accepted = false;
                let mut code = "E_HOST_NAMESPACE_DEFERRED".to_string();
                for tok in &t[2..] {
                    match tok.split_once('=') {
                        Some(("phase", v)) => {
                            phase = v.parse().map_err(|_| format!("alias: bad phase '{v}'"))?
                        }
                        Some(("accepted", v)) => accepted = parse_bool(v)?,
                        Some(("code", v)) => code = v.to_string(),
                        Some((k, _)) => return Err(format!("alias: unknown key '{k}'")),
                        None => {
                            if !resolution.is_empty() {
                                resolution.push(' ');
                            }
                            resolution.push_str(tok);
                        }
                    }
                }
                if resolution.is_empty() {
                    resolution = "same-video-namespace".to_string();
                }
                self.host_aliases.push(HostAlias {
                    host,
                    resolution,
                    phase,
                    accepted,
                    code,
                });
            }
            "entropy" => {
                for tok in &t[1..] {
                    match tok.split_once('=') {
                        Some(("strict", v)) => {
                            self.entropy = if parse_bool(v)? {
                                EntropyMode::Strict
                            } else {
                                EntropyMode::Report
                            }
                        }
                        Some(("mode", v)) => {
                            self.entropy = match v {
                                "off" => EntropyMode::Off,
                                "report" => EntropyMode::Report,
                                "strict" => EntropyMode::Strict,
                                other => return Err(format!("entropy: unknown mode '{other}'")),
                            }
                        }
                        _ => return Err("entropy: expected strict=true|false or mode=off|report|strict".to_string()),
                    }
                }
            }
            "keep-param" => push_unique(&mut self.keep_query_params, arg(&t, 1, "keep-param <name>")?),
            "drop-param" => push_unique(&mut self.drop_query_params, arg(&t, 1, "drop-param <name>")?),
            "segment" => self.apply_segment(&t)?,
            other => return Err(format!("unknown directive '{other}'")),
        }
        Ok(())
    }

    fn apply_segment(&mut self, t: &[&str]) -> Result<(), String> {
        let sub = arg(t, 1, "segment <add|remove|enable|disable|set> ...")?;
        match sub {
            "disable" | "enable" => {
                let id = arg(t, 2, "segment enable|disable <id>")?;
                let seg = self
                    .segments
                    .iter_mut()
                    .find(|s| s.id == id)
                    .ok_or_else(|| format!("segment '{id}' not found"))?;
                seg.enabled = sub == "enable";
            }
            "remove" => {
                let id = arg(t, 2, "segment remove <id>")?;
                let before = self.segments.len();
                self.segments.retain(|s| s.id != id);
                if self.segments.len() == before {
                    return Err(format!("segment '{id}' not found"));
                }
            }
            "add" => {
                let id = arg(t, 2, "segment add <id> <kind> [required=] [severity=] [exact=a|b]")?;
                if self.segments.iter().any(|s| s.id == id) {
                    return Err(format!("segment '{id}' already exists (use 'segment set')"));
                }
                let kind_tok = arg(t, 3, "segment add <id> <kind> ...")?;
                let kind = parse_kind(kind_tok)?;
                let mut rule = SegmentRule {
                    id: id.to_string(),
                    name: kind.label(),
                    kind,
                    matcher: Matcher::Present,
                    required: true,
                    severity: Severity::Error,
                    enabled: true,
                    notes: vec!["added by spec file".to_string()],
                };
                apply_overrides(&mut rule, &t[4..])?;
                self.segments.push(rule);
            }
            "set" => {
                let id = arg(t, 2, "segment set <id> [exact=] [required=] [severity=] [enabled=]")?;
                let rule = self
                    .segments
                    .iter_mut()
                    .find(|s| s.id == id)
                    .ok_or_else(|| format!("segment '{id}' not found"))?;
                apply_overrides(rule, &t[3..])?;
            }
            other => return Err(format!("segment: unknown subcommand '{other}'")),
        }
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        let segments = self
            .segments
            .iter()
            .map(|s| {
                Json::obj(vec![
                    ("id", Json::str(s.id.clone())),
                    ("name", Json::str(s.name.clone())),
                    ("kind", Json::str(s.kind.label())),
                    ("matcher", Json::str(s.matcher.describe())),
                    ("required", Json::Bool(s.required)),
                    ("enabled", Json::Bool(s.enabled)),
                    ("severity", Json::str(s.severity.as_str())),
                    ("notes", Json::strings(s.notes.clone())),
                ])
            })
            .collect::<Vec<_>>();

        let aliases = self
            .host_aliases
            .iter()
            .map(|a| {
                Json::obj(vec![
                    ("host", Json::str(a.host.clone())),
                    ("resolution", Json::str(a.resolution.clone())),
                    ("phase", Json::int(a.phase as i64)),
                    ("accepted", Json::Bool(a.accepted)),
                    ("code", Json::str(a.code.clone())),
                ])
            })
            .collect::<Vec<_>>();

        Json::obj(vec![
            ("specVersion", Json::str(self.spec_version.clone())),
            ("primaryHost", Json::str(self.primary_host.clone())),
            (
                "entropyMode",
                Json::str(match self.entropy {
                    EntropyMode::Off => "off",
                    EntropyMode::Report => "report",
                    EntropyMode::Strict => "strict",
                }),
            ),
            ("hostAliases", Json::arr(aliases)),
            ("keepQueryParams", Json::strings(self.keep_query_params.clone())),
            ("dropQueryParams", Json::strings(self.drop_query_params.clone())),
            (
                "segments",
                Json::arr(segments),
            ),
            (
                "segmentCount",
                Json::int(self.segments.iter().filter(|s| s.enabled).count() as i64),
            ),
        ])
    }
}

fn arg<'a>(t: &[&'a str], idx: usize, usage: &str) -> Result<&'a str, String> {
    t.get(idx)
        .copied()
        .ok_or_else(|| format!("missing argument; usage: {usage}"))
}

fn parse_bool(v: &str) -> Result<bool, String> {
    match v {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        other => Err(format!("expected boolean, got '{other}'")),
    }
}

fn push_unique(v: &mut Vec<String>, item: &str) {
    if !v.iter().any(|x| x == item) {
        v.push(item.to_string());
    }
}

fn parse_kind(s: &str) -> Result<SegmentKind, String> {
    Ok(match s {
        "scheme" => SegmentKind::Scheme,
        "host" => SegmentKind::Host,
        "path" => SegmentKind::Path,
        other => match other.strip_prefix("query:") {
            Some(name) if !name.is_empty() => SegmentKind::QueryParam(name.to_string()),
            _ => {
                return Err(format!(
                    "unknown segment kind '{other}' (scheme|host|path|query:<name>)"
                ))
            }
        },
    })
}

fn apply_overrides(rule: &mut SegmentRule, toks: &[&str]) -> Result<(), String> {
    for tok in toks {
        let (k, v) = match tok.split_once('=') {
            Some(kv) => kv,
            None => return Err(format!("expected key=value, got '{tok}'")),
        };
        match k {
            "exact" => {
                rule.matcher = Matcher::Exact(v.split('|').map(|s| s.to_string()).collect());
            }
            "matcher" => {
                rule.matcher = match v {
                    "present" => Matcher::Present,
                    "videoId" | "videoid" => Matcher::VideoId,
                    "exact" => Matcher::Exact(vec![]),
                    other => return Err(format!("unknown matcher '{other}'")),
                }
            }
            "required" => rule.required = parse_bool(v)?,
            "enabled" => rule.enabled = parse_bool(v)?,
            "severity" => {
                rule.severity = match v {
                    "error" => Severity::Error,
                    "warning" | "warn" => Severity::Warn,
                    "info" => Severity::Info,
                    other => return Err(format!("unknown severity '{other}'")),
                }
            }
            other => return Err(format!("unknown override '{other}'")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_spec_has_the_four_parts() {
        let s = Spec::v1();
        let ids: Vec<&str> = s.segments.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["part-1", "part-2", "part-3", "part-4"]);
        assert_eq!(s.primary_host, "music.youtube.com");
    }

    #[test]
    fn spec_file_can_add_remove_and_retarget() {
        let mut s = Spec::v1();
        s.apply_line("segment remove part-4").unwrap();
        s.apply_line("segment add part-5 query:list required=false severity=warning")
            .unwrap();
        s.apply_line("segment set part-3 exact=/watch|/watch/").unwrap();
        s.apply_line("alias www.youtube.com same-video-namespace accepted=true")
            .unwrap();

        assert_eq!(s.segments.len(), 4);
        assert_eq!(s.segments.last().unwrap().id, "part-5");
        assert!(s.host_aliases.iter().any(|a| a.host == "www.youtube.com" && a.accepted));
        match &s.segments[2].matcher {
            Matcher::Exact(v) => assert_eq!(v.len(), 2),
            _ => panic!("expected exact matcher"),
        }
    }

    #[test]
    fn rejects_unknown_directive() {
        let mut s = Spec::v1();
        assert!(s.apply_line("frobnicate yes").is_err());
        assert!(s.apply_line("segment add part-1 path").is_err()); // duplicate id
    }
}
