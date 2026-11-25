//! kibYtMusicLinkUtil — offline validator for YouTube Music watch links.
//!
//! Phase 1: no sockets, no DNS, no HTTP. Everything below is pure string work
//! plus local file IO (spec in, JSON report out).

mod cli;
mod json;
mod output;
mod report;
mod segments;
mod spec;
mod timefmt;
mod urlparse;
mod version;
mod videoid;

use std::io::Read;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match cli::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n");
            eprint!("{}", cli::USAGE);
            process::exit(2);
        }
    };

    if opts.help {
        print!("{}", cli::USAGE);
        return;
    }
    if opts.version {
        println!("{} {}", version::NAME, version::VERSION);
        return;
    }

    let mut spec = spec::Spec::v1();
    if let Some(path) = &opts.spec_file {
        if let Err(e) = spec.load_file(path) {
            eprintln!("error: spec: {e}");
            process::exit(2);
        }
    }
    if opts.strict_entropy {
        spec.entropy = videoid::EntropyMode::Strict;
    } else if opts.no_entropy {
        spec.entropy = videoid::EntropyMode::Off;
    }

    let inputs = match collect_inputs(&opts) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    let mut results: Vec<json::Json> = Vec::with_capacity(inputs.len());
    let mut lines: Vec<String> = Vec::with_capacity(inputs.len());
    let (mut n_valid, mut n_warn, mut n_invalid) = (0usize, 0usize, 0usize);

    for (i, (source, raw)) in inputs.iter().enumerate() {
        let env = report::build_result(i + 1, source, raw, &spec);
        match env.verdict.as_str() {
            "valid" => n_valid += 1,
            "valid_with_warnings" => n_warn += 1,
            _ => n_invalid += 1,
        }
        if opts.verbose {
            for d in &env.details {
                eprintln!("        {d}");
            }
        }
        lines.push(env.line);
        results.push(env.json);
    }

    let epoch = timefmt::now_epoch_secs();
    let aggregate = json::Json::obj(vec![
        (
            "tool",
            json::Json::obj(vec![
                ("name", json::Json::str(version::NAME)),
                ("version", json::Json::str(version::VERSION)),
                ("phase", json::Json::int(version::PHASE as i64)),
                ("network", json::Json::Bool(version::NETWORK)),
            ]),
        ),
        ("generatedAt", json::Json::str(timefmt::rfc3339_from_epoch(epoch))),
        ("generatedAtEpoch", json::Json::int(epoch)),
        ("spec", spec.to_json()),
        (
            "summary",
            json::Json::obj(vec![
                ("inputs", json::Json::int(inputs.len() as i64)),
                ("valid", json::Json::int(n_valid as i64)),
                ("validWithWarnings", json::Json::int(n_warn as i64)),
                ("invalid", json::Json::int(n_invalid as i64)),
            ]),
        ),
        ("results", json::Json::arr(results)),
    ]);

    let body = aggregate.to_pretty();
    let filename = format!("report-{}.json", timefmt::compact_stamp(epoch));
    let path = match output::write(&opts.out_dir, &filename, &body) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: writing report: {e}");
            process::exit(2);
        }
    };

    if !opts.quiet {
        println!(
            "{} {} — phase {} (local only, no network)",
            version::NAME,
            version::VERSION,
            version::PHASE
        );
        println!(
            "spec {} · {} segment(s) · host lock '{}' · entropy {:?}",
            spec.spec_version,
            spec.segments.iter().filter(|s| s.enabled).count(),
            spec.primary_host,
            spec.entropy
        );
        println!();
        for l in &lines {
            println!("{l}");
        }
        println!();
        println!(
            "summary: {} valid, {} with warnings, {} invalid",
            n_valid, n_warn, n_invalid
        );
        println!("report:  {}", path.display());
    }
    if opts.print_json {
        println!("{body}");
    }

    process::exit(if n_invalid > 0 { 1 } else { 0 });
}

/// (source, raw) pairs. Source is one of `argv` | `file` | `stdin` and is
/// recorded in the report so you can tell where a bad link came from.
fn collect_inputs(opts: &cli::Options) -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();

    for a in &opts.inputs {
        out.push(("argv".to_string(), a.clone()));
    }

    if let Some(path) = &opts.input_file {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        push_lines(&mut out, "file", &text);
    }

    let want_stdin =
        opts.read_stdin || (opts.inputs.is_empty() && opts.input_file.is_none());
    if want_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("cannot read stdin: {e}"))?;
        push_lines(&mut out, "stdin", &buf);
    }

    if out.is_empty() {
        return Err(
            "no input supplied (pass URLs as arguments, use --input-file, or pipe via stdin)"
                .to_string(),
        );
    }
    Ok(out)
}

fn push_lines(out: &mut Vec<(String, String)>, source: &str, text: &str) {
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        out.push((source.to_string(), t.to_string()));
    }
}
