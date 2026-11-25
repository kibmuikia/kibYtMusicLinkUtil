//! Argument parsing. Hand-rolled: the surface is small and stable, and a
//! zero-dependency build keeps the binary auditable.

use std::path::PathBuf;

pub const USAGE: &str = "\
kibytm — kibYtMusicLinkUtil: offline YouTube Music link validator (v0.0.1)

USAGE:
    kibytm [OPTIONS] [URL|STRING]...

OPTIONS:
    -h, --help                 show this help
    -V, --version              show version
        --stdin                also read newline-separated inputs from stdin
        --input-file <PATH>    read newline-separated inputs from a file
        --out-dir <PATH>       report directory (default: ./output)
        --spec <PATH>          spec file: add/remove/retarget segments
    -v, --verbose              print per-segment checks to stderr
    -q, --quiet                suppress the human summary
        --print                also print the aggregate report JSON to stdout
        --strict-entropy       make the videoId entropy check fatal
        --no-entropy-check     skip the videoId entropy check

INPUTS:
    Each argument is validated independently. With no arguments, stdin is read
    (one input per line, '#' comments allowed). No network requests are made.

EXIT CODES:
    0 all inputs valid (warnings allowed)   1 at least one invalid   2 usage/IO error
";

pub struct Options {
    pub inputs: Vec<String>,
    pub read_stdin: bool,
    pub input_file: Option<PathBuf>,
    pub out_dir: PathBuf,
    pub spec_file: Option<PathBuf>,
    pub print_json: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub help: bool,
    pub version: bool,
    pub strict_entropy: bool,
    pub no_entropy: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            inputs: Vec::new(),
            read_stdin: false,
            input_file: None,
            out_dir: PathBuf::from("output"),
            spec_file: None,
            print_json: false,
            verbose: false,
            quiet: false,
            help: false,
            version: false,
            strict_entropy: false,
            no_entropy: false,
        }
    }
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut only_positional = false;
    let mut i = 0;

    while i < args.len() {
        let a = args[i].as_str();
        if only_positional {
            o.inputs.push(a.to_string());
            i += 1;
            continue;
        }
        match a {
            "--" => only_positional = true,
            "-h" | "--help" => o.help = true,
            "-V" | "--version" => o.version = true,
            "--stdin" => o.read_stdin = true,
            "--print" => o.print_json = true,
            "-v" | "--verbose" => o.verbose = true,
            "-q" | "--quiet" => o.quiet = true,
            "--strict-entropy" => o.strict_entropy = true,
            "--no-entropy-check" => o.no_entropy = true,
            "--input-file" | "--out-dir" | "--spec" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| format!("{a} requires a value"))?
                    .clone();
                match a {
                    "--input-file" => o.input_file = Some(PathBuf::from(v)),
                    "--out-dir" => o.out_dir = PathBuf::from(v),
                    _ => o.spec_file = Some(PathBuf::from(v)),
                }
            }
            s if s.starts_with("--input-file=") => {
                o.input_file = Some(PathBuf::from(&s["--input-file=".len()..]))
            }
            s if s.starts_with("--out-dir=") => {
                o.out_dir = PathBuf::from(&s["--out-dir=".len()..])
            }
            s if s.starts_with("--spec=") => o.spec_file = Some(PathBuf::from(&s["--spec=".len()..])),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option '{s}'")),
            s => o.inputs.push(s.to_string()),
        }
        i += 1;
    }

    if o.strict_entropy && o.no_entropy {
        return Err("--strict-entropy and --no-entropy-check are mutually exclusive".to_string());
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_positional_and_equals_form() {
        let o = parse(&v(&[
            "https://music.youtube.com/watch?v=qARrn7G067w",
            "--out-dir=reports",
            "--print",
            "-v",
        ]))
        .unwrap();
        assert_eq!(o.inputs.len(), 1);
        assert_eq!(o.out_dir, PathBuf::from("reports"));
        assert!(o.print_json && o.verbose);
    }

    #[test]
    fn rejects_bad_usage() {
        assert!(parse(&v(&["--nope"])).is_err());
        assert!(parse(&v(&["--spec"])).is_err());
        assert!(parse(&v(&["--strict-entropy", "--no-entropy-check"])).is_err());
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let o = parse(&v(&["--", "-weird-input"])).unwrap();
        assert_eq!(o.inputs, vec!["-weird-input".to_string()]);
    }
}
