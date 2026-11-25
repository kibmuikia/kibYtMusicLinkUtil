//! Turns one raw input into one report object (JSON) plus the human line we
//! print on stdout. All validation evidence lives in the JSON, not the printout.

use crate::json::Json;
use crate::segments::{self, Status};
use crate::spec::Spec;
use crate::urlparse::{self, InputShape};
use crate::version;
use crate::videoid::VideoIdValidation;

pub struct Envelope {
    pub json: Json,
    pub verdict: String,
    pub line: String,
    pub details: Vec<String>,
}

pub fn build_result(index: usize, source: &str, raw: &str, spec: &Spec) -> Envelope {
    let parsed = urlparse::parse(raw);
    let outcomes = segments::evaluate(&parsed, spec);

    let mut errors: Vec<Json> = Vec::new();
    let mut warnings: Vec<Json> = Vec::new();
    let mut details: Vec<String> = Vec::new();
    let mut primary_error: Option<String> = None;

    for o in &outcomes {
        details.push(format!(
            "{:<7} {:<6} {:<20} {:<28} {}",
            o.id,
            o.status.as_str(),
            o.name,
            o.code,
            o.message
        ));
        match o.status {
            Status::Fail => {
                errors.push(o.problem_json());
                if primary_error.is_none() {
                    primary_error = Some(format!("{}: {}", o.code, o.message));
                }
            }
            Status::Warn => warnings.push(o.problem_json()),
            _ => {}
        }
    }

    for p in &parsed.problems {
        warnings.push(Json::obj(vec![
            ("code", Json::str("W_PARSE")),
            ("segment", Json::Null),
            ("message", Json::str(p.clone())),
        ]));
        details.push(format!("parse   warn   {:<34} {}", "-", p));
    }

    let video_id: Option<VideoIdValidation> = outcomes.iter().find_map(|o| o.video_id.clone());

    let verdict = if !errors.is_empty() {
        "invalid"
    } else if !warnings.is_empty() {
        "valid_with_warnings"
    } else {
        "valid"
    };

    // ---- normalization ------------------------------------------------
    let v_pair = parsed.query.iter().find(|p| p.key == "v");
    let canonical = match (&parsed.host, &parsed.path, v_pair) {
        (Some(h), Some(p), Some(q)) if !q.value.is_empty() => {
            Some(format!("https://{h}{p}?v={}", q.value))
        }
        _ => None,
    };

    let mut kept_params: Vec<Json> = Vec::new();
    let mut dropped_params: Vec<String> = Vec::new();
    for p in &parsed.query {
        if spec.keep_query_params.iter().any(|k| k == &p.key) {
            kept_params.push(p.to_json());
        } else {
            if !dropped_params.contains(&p.key) {
                dropped_params.push(p.key.clone());
            }
        }
    }

    let mut alternates: Vec<(&str, Json)> = Vec::new();
    if let Some(v) = &video_id {
        if v.valid {
            let id = &v.candidate;
            alternates.push(("musicWatch", Json::str(format!("https://music.youtube.com/watch?v={id}"))));
            alternates.push(("mainHostWatch", Json::str(format!("https://www.youtube.com/watch?v={id}"))));
            alternates.push(("short", Json::str(format!("https://youtu.be/{id}"))));
            alternates.push(("embed", Json::str(format!("https://www.youtube.com/embed/{id}"))));
            alternates.push((
                "thumbnail",
                Json::str(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
            ));
            alternates.push((
                "thumbnailMaxRes",
                Json::str(format!("https://i.ytimg.com/vi/{id}/maxresdefault.jpg")),
            ));
            alternates.push(("oembedEndpoint", Json::str(format!(
                "https://www.youtube.com/oembed?url=https://www.youtube.com/watch?v={id}&format=json"
            ))));
        }
    }

    // ---- hints --------------------------------------------------------
    let mut hints: Vec<String> = Vec::new();
    if parsed.shape == InputShape::BareVideoId {
        hints.push(
            "input is a bare videoId; wrap it as https://music.youtube.com/watch?v=<id> or accept it in phase 2"
                .to_string(),
        );
    }
    if outcomes.iter().any(|o| o.code == "E_HOST_NAMESPACE_DEFERRED") {
        hints.push(
            "phase 2 will resolve the music/www namespace: a spec line 'alias www.youtube.com accepted=true' already enables it today"
                .to_string(),
        );
    }
    if outcomes.iter().any(|o| o.code == "E_HOST_SHORTENER_DEFERRED") {
        hints.push(
            "youtu.be short links need path-based id extraction; phase 2 (spec: 'segment add part-3 path exact=/<id>' style handling)"
                .to_string(),
        );
    }
    if verdict != "invalid" && !dropped_params.is_empty() {
        hints.push(format!(
            "share residue present ({}); persist the canonical form instead",
            dropped_params.join(", ")
        ));
    }

    let label = video_id
        .as_ref()
        .map(|v| v.candidate.clone())
        .unwrap_or_else(|| "-".to_string());

    let line = match &primary_error {
        Some(e) => format!("[{index}] {:<7} {:<12} {}\n           -> {}", "invalid", label, raw, e),
        None => format!("[{index}] {:<7} {:<12} {}", verdict, label, raw),
    };

    let json = Json::obj(vec![
        ("index", Json::int(index as i64)),
        ("source", Json::str(source.to_string())),
        (
            "input",
            Json::obj(vec![
                ("raw", Json::str(parsed.raw.clone())), // parser-cleaned provenance, not the argv echo
                ("shape", Json::str(parsed.shape.as_str())),
            ]),
        ),
        (
            "parse",
            Json::obj(vec![
                ("scheme", Json::opt_str(parsed.scheme.clone())),
                ("host", Json::opt_str(parsed.host.clone())),
                ("port", Json::opt_str(parsed.port.clone())),
                ("path", Json::opt_str(parsed.path.clone())),
                ("fragment", Json::opt_str(parsed.fragment.clone())),
                ("userinfo", Json::opt_str(parsed.userinfo.clone())),
                (
                    "query",
                    Json::arr(parsed.query.iter().map(|p| p.to_json())),
                ),
                ("problems", Json::strings(parsed.problems.clone())),
            ]),
        ),
        (
            "segments",
            Json::arr(outcomes.iter().map(|o| o.to_json())),
        ),
        (
            "videoId",
            match &video_id {
                Some(v) => v.to_json(),
                None => Json::Null,
            },
        ),
        (
            "normalization",
            Json::obj(vec![
                ("canonical", Json::opt_str(canonical.clone())),
                ("keptParams", Json::arr(kept_params)),
                ("droppedParams", Json::strings(dropped_params.clone())),
                ("dropIsSafe", Json::Bool(true)),
                ("alternateForms", Json::obj(alternates.clone())),
            ]),
        ),
        (
            "verdict",
            Json::obj(vec![
                ("status", Json::str(verdict)),
                ("errors", Json::arr(errors)),
                ("warnings", Json::arr(warnings)),
            ]),
        ),
        ("hints", Json::strings(hints)),
        (
            "next",
            Json::obj(vec![
                ("onlineConfirmation", Json::str("deferred")),
                ("phase", Json::int(version::PHASE as i64)),
                (
                    "reason",
                    Json::str("phase 1 is local-only: no network requests are made"),
                ),
            ]),
        ),
    ]);

    Envelope {
        json,
        verdict: verdict.to_string(),
        line,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_link_is_valid_and_normalized() {
        let spec = Spec::v1();
        let env = build_result(
            1,
            "argv",
            "https://music.youtube.com/watch?v=qARrn7G067w&si=abc",
            &spec,
        );
        assert_eq!(env.verdict, "valid"); // `si` is share residue: dropped, not a warning
        assert!(env.line.contains("qARrn7G067w")); // label lives in the human line only
        let s = env.json.to_pretty();
        assert!(s.contains("\"canonical\": \"https://music.youtube.com/watch?v=qARrn7G067w\""));
        assert!(s.contains("youtu.be/qARrn7G067w"));
    }

    #[test]
    fn http_link_is_invalid() {
        let spec = Spec::v1();
        let env = build_result(1, "argv", "http://music.youtube.com/watch?v=qARrn7G067w", &spec);
        assert_eq!(env.verdict, "invalid");
    }
}
