//! Segment engine: walks the spec and produces one auditable outcome per
//! segment. The engine never hard-codes part numbers — everything comes from
//! `Spec::segments`, so add/remove/retarget is a data change.

use crate::json::Json;
use crate::spec::{Matcher, SegmentKind, SegmentRule, Spec};
use crate::urlparse::{ParsedInput, QueryPair};
use crate::videoid::{self, VideoIdValidation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skipped,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SegmentOutcome {
    pub id: String,
    pub name: String,
    pub kind_label: String,
    pub required: bool,
    pub enabled: bool,
    pub status: Status,
    pub code: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: String,
    pub notes: Vec<String>,
    pub video_id: Option<VideoIdValidation>,
}

impl SegmentOutcome {
    fn base(rule: &SegmentRule) -> SegmentOutcome {
        SegmentOutcome {
            id: rule.id.clone(),
            name: rule.name.clone(),
            kind_label: rule.kind.label(),
            required: rule.required,
            enabled: rule.enabled,
            status: Status::Skipped,
            code: String::new(),
            expected: None,
            actual: None,
            message: String::new(),
            notes: rule.notes.clone(),
            video_id: None,
        }
    }

    fn skipped(rule: &SegmentRule, why: &str) -> SegmentOutcome {
        let mut o = SegmentOutcome::base(rule);
        o.code = "SEG_DISABLED".to_string();
        o.message = why.to_string();
        o
    }

    pub fn problem_json(&self) -> Json {
        Json::obj(vec![
            ("code", Json::str(self.code.clone())),
            ("segment", Json::str(self.id.clone())),
            ("segmentName", Json::str(self.name.clone())),
            ("severity", Json::str(self.status.as_str())),
            ("message", Json::str(self.message.clone())),
        ])
    }

    pub fn to_json(&self) -> Json {
        let mut pairs: Vec<(&str, Json)> = vec![
            ("id", Json::str(self.id.clone())),
            ("name", Json::str(self.name.clone())),
            ("kind", Json::str(self.kind_label.clone())),
            ("required", Json::Bool(self.required)),
            ("enabled", Json::Bool(self.enabled)),
            ("status", Json::str(self.status.as_str())),
            ("code", Json::str(self.code.clone())),
            ("expected", Json::opt_str(self.expected.clone())),
            ("actual", Json::opt_str(self.actual.clone())),
            ("message", Json::str(self.message.clone())),
            ("notes", Json::strings(self.notes.clone())),
        ];
        if let Some(v) = &self.video_id {
            pairs.push(("videoIdValidation", v.to_json()));
        }
        Json::obj(pairs)
    }
}

pub fn evaluate(input: &ParsedInput, spec: &Spec) -> Vec<SegmentOutcome> {
    let mut out = Vec::with_capacity(spec.segments.len());
    for rule in &spec.segments {
        if !rule.enabled {
            out.push(SegmentOutcome::skipped(rule, "segment disabled by spec"));
            continue;
        }
        let outcome = match &rule.kind {
            SegmentKind::Scheme => eval_scheme(rule, input),
            SegmentKind::Host => eval_host(rule, input, spec),
            SegmentKind::Path => eval_path(rule, input),
            SegmentKind::QueryParam(name) => eval_query(rule, name, input, spec),
        };
        out.push(outcome);
    }
    out
}

fn eval_scheme(rule: &SegmentRule, input: &ParsedInput) -> SegmentOutcome {
    let mut o = SegmentOutcome::base(rule);
    o.expected = Some(rule.matcher.describe());
    o.actual = input.scheme.clone();
    match &input.scheme {
        None => {
            o.status = Status::Fail;
            o.code = "E_SCHEME_MISSING".to_string();
            o.message = "no scheme found; the input must start with 'https://'".to_string();
        }
        Some(s) => {
            if rule.matcher.matches(s) {
                o.status = Status::Pass;
                o.code = "SEG_OK".to_string();
                o.message = format!("scheme '{s}' accepted");
            } else {
                o.status = Status::Fail;
                o.code = "E_SCHEME_MISMATCH".to_string();
                o.message = format!(
                    "scheme '{s}' rejected; expected 'https' (http is 301-upgraded by YouTube but is not accepted here)"
                );
            }
        }
    }
    o
}

fn eval_host(rule: &SegmentRule, input: &ParsedInput, spec: &Spec) -> SegmentOutcome {
    let mut o = SegmentOutcome::base(rule);
    o.expected = Some(rule.matcher.describe());
    o.actual = input.host.clone();

    let host = match &input.host {
        None => {
            o.status = Status::Fail;
            o.code = "E_HOST_MISSING".to_string();
            o.message = "no host found in the input".to_string();
            return o;
        }
        Some(h) => h.clone(),
    };

    if rule.matcher.matches(&host) {
        o.status = Status::Pass;
        o.code = "SEG_OK".to_string();
        o.message = format!("host '{host}' is the locked primary host");
        return o;
    }

    if let Some(alias) = spec
        .host_aliases
        .iter()
        .find(|a| a.host.eq_ignore_ascii_case(&host))
    {
        if alias.accepted {
            o.status = Status::Pass;
            o.code = "SEG_HOST_ALIAS".to_string();
            o.message = format!(
                "host '{host}' accepted as an alias of '{}' ({})",
                spec.primary_host, alias.resolution
            );
            o.notes
                .push(format!("resolved to primary host '{}'", spec.primary_host));
        } else {
            o.status = Status::Fail;
            o.code = alias.code.clone();
            o.message = format!(
                "host '{host}' is known ({}), but phase {} of this tool accepts only '{}'",
                alias.resolution, alias.phase, spec.primary_host
            );
            o.notes.push(format!(
                "namespace: '{host}' and '{}' share one videoId space — the same id resolves to the same asset",
                spec.primary_host
            ));
            if alias.resolution == "shortener" {
                o.notes.push(
                    "short links carry the id in the path ('youtu.be/<id>'), not in '?v='".to_string(),
                );
            }
        }
        return o;
    }

    o.status = Status::Fail;
    o.code = "E_HOST_MISMATCH".to_string();
    o.message = format!(
        "host '{host}' rejected; expected '{}'",
        spec.primary_host
    );
    o
}

fn eval_path(rule: &SegmentRule, input: &ParsedInput) -> SegmentOutcome {
    let mut o = SegmentOutcome::base(rule);
    o.expected = Some(rule.matcher.describe());
    o.actual = input.path.clone();

    let path = match &input.path {
        None => {
            o.status = Status::Fail;
            o.code = "E_PATH_MISSING".to_string();
            o.message = "no path found; expected '/watch'".to_string();
            return o;
        }
        Some(p) => p.clone(),
    };

    if rule.matcher.matches(&path) {
        o.status = Status::Pass;
        o.code = "SEG_OK".to_string();
        o.message = format!("path '{path}' accepted");
        return o;
    }

    o.status = Status::Fail;
    o.code = if path.trim_end_matches('/') == "/watch" {
        "E_PATH_TRAILING_SLASH".to_string()
    } else if path.starts_with("/embed") || path.starts_with("/v/") {
        "E_PATH_EMBED_ROUTE".to_string()
    } else {
        "E_PATH_MISMATCH".to_string()
    };
    o.message = format!("path '{path}' rejected; expected '/watch'");
    if o.code == "E_PATH_EMBED_ROUTE" {
        o.notes
            .push("embed routes carry the id in the path, not in '?v='".to_string());
    }
    o
}

fn eval_query(
    rule: &SegmentRule,
    name: &str,
    input: &ParsedInput,
    spec: &Spec,
) -> SegmentOutcome {
    let mut o = SegmentOutcome::base(rule);
    o.expected = Some(format!("?{name}=<videoId>"));

    let found: Vec<&QueryPair> = input.query.iter().filter(|p| p.key == name).collect();
    if found.is_empty() {
        o.status = Status::Fail;
        o.code = "E_PARAM_MISSING".to_string();
        o.message = format!("query parameter '{name}' (videoId) is absent");
        if !input.query.is_empty() {
            let present: Vec<String> = input.query.iter().map(|p| p.key.clone()).collect();
            o.notes
                .push(format!("parameters present: {}", present.join(", ")));
        }
        return o;
    }

    if found.len() > 1 {
        o.notes.push(format!(
            "parameter '{name}' appears {} times; the first occurrence is used",
            found.len()
        ));
    }

    let pair = found[0];
    o.actual = Some(pair.value_raw.clone());

    // A spec-added query segment may not want video-id semantics at all.
    match &rule.matcher {
        Matcher::Present => {
            o.status = Status::Pass;
            o.code = "SEG_OK".to_string();
            o.message = format!("parameter '{name}' is present");
            return o;
        }
        Matcher::Exact(list) if !list.is_empty() => {
            let hit = found
                .iter()
                .any(|p| list.iter().any(|e| e.eq_ignore_ascii_case(&p.value)));
            o.status = if hit { Status::Pass } else { Status::Fail };
            o.code = if hit {
                "SEG_OK".to_string()
            } else {
                "E_PARAM_VALUE_MISMATCH".to_string()
            };
            o.message = format!(
                "parameter '{name}' = '{}'; expected one of: {}",
                pair.value,
                list.join(" | ")
            );
            return o;
        }
        _ => {}
    }

    if !spec.keep_query_params.iter().any(|k| k == name) {
        o.notes.push(format!(
            "'{name}' is not listed in keepQueryParams of the spec; canonical form will drop it"
        ));
    }

    let validation = videoid::validate(&pair.value_raw, spec.entropy);
    let valid = validation.valid;
    let soft_failure = validation.has_soft_failure();
    let failed: Vec<String> = validation
        .failed_errors()
        .iter()
        .map(|c| c.code.to_string())
        .collect();

    if !valid {
        o.status = Status::Fail;
        o.code = "E_VIDEO_ID_INVALID".to_string();
        o.message = format!(
            "videoId '{}' failed offline validation: {}",
            validation.candidate,
            failed.join(", ")
        );
    } else if soft_failure {
        o.status = Status::Warn;
        o.code = "W_VIDEO_ID_SOFT_CHECK".to_string();
        o.message = format!(
            "videoId '{}' passed every hard check; a soft check flagged it (see videoIdValidation)",
            validation.candidate
        );
    } else {
        o.status = Status::Pass;
        o.code = "SEG_OK".to_string();
        o.message = format!(
            "videoId '{}' valid ({} checks applied)",
            validation.candidate,
            validation.checks.len()
        );
    }

    if !pair.decoded_ok {
        o.notes
            .push("malformed percent-escape in the query value; raw value kept".to_string());
    }
    if found.len() > 1 && o.status == Status::Pass {
        o.status = Status::Warn;
        o.code = "W_PARAM_V_DUPLICATE".to_string();
    }

    o.video_id = Some(validation);
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::urlparse;

    fn run(url: &str) -> Vec<SegmentOutcome> {
        let spec = Spec::v1();
        evaluate(&urlparse::parse(url), &spec)
    }

    #[test]
    fn sample_links_pass_every_segment() {
        for url in [
            "https://music.youtube.com/watch?v=qARrn7G067w",
            "https://music.youtube.com/watch?v=AHsnSK0quvc",
        ] {
            let out = run(url);
            let bad: Vec<&str> = out
                .iter()
                .filter(|o| o.status == Status::Fail || o.status == Status::Warn)
                .map(|o| o.code.as_str())
                .collect();
            assert!(bad.is_empty(), "{url} -> {bad:?}");
        }
    }

    #[test]
    fn scheme_must_be_https() {
        let out = run("http://music.youtube.com/watch?v=qARrn7G067w");
        assert_eq!(out[0].status, Status::Fail);
        assert_eq!(out[0].code, "E_SCHEME_MISMATCH");
    }

    #[test]
    fn www_and_shortener_are_known_but_deferred() {
        let out = run("https://www.youtube.com/watch?v=qARrn7G067w");
        assert_eq!(out[1].code, "E_HOST_NAMESPACE_DEFERRED");

        let out = run("https://youtu.be/qARrn7G067w");
        assert_eq!(out[1].code, "E_HOST_SHORTENER_DEFERRED");
        assert_eq!(out[2].code, "E_PATH_MISMATCH");
    }

    #[test]
    fn path_and_param_errors() {
        let out = run("https://music.youtube.com/watch/?v=qARrn7G067w");
        assert_eq!(out[2].code, "E_PATH_TRAILING_SLASH");

        let out = run("https://music.youtube.com/watch?list=RD");
        assert_eq!(out[3].code, "E_PARAM_MISSING");

        let out = run("https://music.youtube.com/watch?v=qARrn7G067");
        assert_eq!(out[3].code, "E_VIDEO_ID_INVALID");
    }

    #[test]
    fn disabled_segment_is_skipped_not_failed() {
        let mut spec = Spec::v1();
        spec.apply_line("segment disable part-3").unwrap();
        let out = evaluate(&urlparse::parse("https://music.youtube.com/watch?v=qARrn7G067w"), &spec);
        assert_eq!(out[2].status, Status::Skipped);
        assert_eq!(out[2].code, "SEG_DISABLED");
    }
}
