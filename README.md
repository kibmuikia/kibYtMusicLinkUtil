# kibYtMusicLinkUtil

Offline validator + normalizer for YouTube Music watch links. Part of the
link-intake pipeline for the music/YouTube project.

**Phase 1 — local only.** No sockets, no DNS, no HTTP. Given a URL or string it
decomposes the link into its four parts, checks each against a *data-driven
spec*, validates the video id offline, and writes a JSON report to `output/`.

- Binary: `kibytm`
- Version: `0.0.1`
- Spec version: `1.0`
- Dependencies: **none** (builds air-gapped, auditable binary)

---

## The four parts

| Part | Segment | v1 rule | Failure codes |
|------|---------|---------|---------------|
| part-1 | scheme | exactly `https` | `E_SCHEME_MISSING`, `E_SCHEME_MISMATCH` |
| part-2 | host | exactly `music.youtube.com` | `E_HOST_MISSING`, `E_HOST_MISMATCH`, `E_HOST_NAMESPACE_DEFERRED`, `E_HOST_SHORTENER_DEFERRED` |
| part-3 | path | exactly `/watch` | `E_PATH_MISSING`, `E_PATH_MISMATCH`, `E_PATH_TRAILING_SLASH`, `E_PATH_EMBED_ROUTE` |
| part-4 | query `v` | valid video id | `E_PARAM_MISSING`, `E_VIDEO_ID_INVALID`, `W_PARAM_V_DUPLICATE` |

### Namespace readiness (host)

`music.youtube.com` and `www.youtube.com` share one videoId space — the same id
is the same asset, only the client differs. v1 still rejects the main host, but
rejects it *differently*: `E_HOST_NAMESPACE_DEFERRED` (known alias, phase 2)
rather than `E_HOST_MISMATCH` (unknown host). `youtu.be` gets
`E_HOST_SHORTENER_DEFERRED` because it carries the id in the path, not in `?v=`.
Flipping the switch later is one spec line, no code change:

```
alias www.youtube.com same-video-namespace accepted=true
```

### videoId checks (part-4, all offline)

| Code | Severity | Rule |
|------|----------|------|
| `VID_NOT_EMPTY` | error | value is non-empty |
| `VID_LENGTH_EQ_11` | error | exactly 11 chars (canonical; de-facto, not contractual) |
| `VID_CHARSET_URLSAFE_BASE64` | error | chars from `[A-Za-z0-9_-]` |
| `VID_NO_PERCENT_ENCODING` | error | no `%` (catches double-encoded / truncated params) |
| `VID_NO_BASE64_PADDING` | error | no `=`, `+`, `/` (standard-alphabet leakage) |
| `VID_NO_WHITESPACE_OR_QUOTES` | error | no whitespace/quotes/angle brackets |
| `VID_LAST_CHAR_ENTROPY` | warning¹ | last char index multiple of 4 — a 64-bit id in 11 base64 chars leaves the final 6-bit group with only 4 significant bits, so only 16 of 64 characters are legal there |
| `VID_CASE_SENSITIVE` | info | ids are case-sensitive; never lowercase |

¹ `--strict-entropy` promotes it to an error, `--no-entropy-check` disables it.
Default is *report*: the rule holds for every id observed so far, but it is not
a documented contract, so it must not break the pipeline silently.

---

## Layout

```
src/
  main.rs       wiring: args → spec → inputs → reports → file
  cli.rs        hand-rolled arg parsing (no clap)
  spec.rs       segment rules + line DSL + built-in v1 policy
  segments.rs   engine: one auditable outcome per segment
  urlparse.rs   scheme/authority/path/query/fragment decomposition
  videoid.rs    videoId checks, severities, report shape
  report.rs     per-input JSON report + human line
  json.rs       minimal JSON writer (no serde)
  timefmt.rs    UTC timestamps (no chrono)
  output.rs     writes output/*.json (git-ignored)
  version.rs    name / version / phase / spec version
examples/
  phase2.spec   spec that accepts the www namespace + youtu.be
  inputs.txt    sample input file
```

`output/` is created on demand and is in `.gitignore`.

## Build & run

```bash
cargo build --release
cargo test

# one or many inputs
./target/release/kibytm "https://music.youtube.com/watch?v=qARrn7G067w"

# file / stdin
./target/release/kibytm --input-file examples/inputs.txt -v
printf '%s\n' "https://music.youtube.com/watch?v=AHsnSK0quvc" | ./target/release/kibytm

# custom spec, custom report dir, JSON on stdout too
./target/release/kibytm --spec examples/phase2.spec --out-dir /tmp/rep --print \
  "https://www.youtube.com/watch?v=qARrn7G067w"
```

Exit codes: `0` all valid (warnings allowed), `1` at least one invalid, `2` usage/IO error.

## Report

One file per run: `output/report-<YYYYMMDDTHHMMSSZ>.json`. An array of results,
each carrying the full evidence trail:

```json
{
  "tool": { "name": "kibYtMusicLinkUtil", "version": "0.0.1", "phase": 1, "network": false },
  "generatedAt": "2026-11-26T01:33:00Z",
  "spec": { "primaryHost": "music.youtube.com", "segments": [] },
  "summary": { "inputs": 2, "valid": 2, "validWithWarnings": 0, "invalid": 0 },
  "results": [
    {
      "input":  { "raw": "https://music.youtube.com/watch?v=qARrn7G067w", "shape": "url" },
      "parse":  { "scheme": "https", "host": "music.youtube.com", "path": "/watch", "query": [] },
      "segments": [ { "id": "part-1", "status": "pass", "code": "SEG_OK" } ],
      "videoId": {
        "value": "qARrn7G067w", "valid": true, "length": 11,
        "validationTypes": ["structural", "length", "charset", "encoding", "entropy", "semantics"],
        "checksApplied": 8,
        "checks": [ { "code": "VID_LAST_CHAR_ENTROPY", "severity": "warning", "passed": true } ]
      },
      "normalization": {
        "canonical": "https://music.youtube.com/watch?v=qARrn7G067w",
        "droppedParams": ["si"],
        "alternateForms": { "short": "https://youtu.be/qARrn7G067w" }
      },
      "verdict": { "status": "valid", "errors": [], "warnings": [] },
      "next": { "onlineConfirmation": "deferred", "phase": 1 }
    }
  ]
}
```

(abridged — every segment outcome, note and parse problem is present in the real file)

## Spec DSL

Flags patch the built-in v1 spec at startup. One directive per line, `#` comments.

```
primary-host music.youtube.com
spec-version 1.1
alias www.youtube.com same-video-namespace phase=2 accepted=false code=E_HOST_NAMESPACE_DEFERRED
alias youtu.be shortener phase=2 accepted=false code=E_HOST_SHORTENER_DEFERRED
entropy strict=false            # or: entropy mode=off|report|strict
keep-param v
drop-param si
segment disable part-3
segment enable part-3
segment remove part-4
segment add part-5 query:list required=false severity=warning
segment add part-6 path exact=/watch|/watch/
segment set part-2 exact=music.youtube.com|music.youtube.com.
```

Matchers: `exact=a|b` (case-insensitive), `matcher=present`, `matcher=videoId`.
Overrides: `required=`, `enabled=`, `severity=error|warning|info`.

## Design notes / tradeoffs

- **Zero dependencies** (no serde, no url, no regex, no clap). Cost: hand-written
  JSON writer and tokenizer. Benefit: `cargo build` works with the network off,
  no supply-chain surface, ~300 KB binary, and full control over the component
  model that *is* the product surface. If you later want a general URL parser,
  swap `urlparse.rs` behind the same `ParsedInput` struct.
- **Segments are data, not code.** The engine walks `Vec<SegmentRule>`; adding a
  segment is a spec line, not a match arm. Everything reports `id`/`code`/
  `message` so archived reports stay interpretable when the spec moves.
- **Errors vs warnings vs soft checks.** Only `Severity::Error` failures set
  `verdict=invalid`. Parse oddities and the entropy rule are warnings — a link
  pipeline that hard-fails on speculative rules is worse than one that logs.
- **Canonicalization is offered, not applied.** Report gives `canonical` plus
  `droppedParams`; the caller decides whether to rewrite the stored link.
- Not a general URL parser: IPv6 literals and userinfo are handled defensively
  and flagged, not supported. Fragments are rejected as a `v` carrier.

## Phase 2 backlog

1. Accept `www.youtube.com` / `youtube.com` / `m.youtube.com` as namespace aliases
   and canonicalize onto one host (spec-only, already wired).
2. `youtu.be/<id>` and `/embed/<id>`, `/v/<id>`, `/shorts/<id>` path-based id
   extraction (needs a `PathCarriesId` matcher).
3. `query:t` / `query:start` timestamp parsing and `t=90s` canonicalization.
4. Batch mode with dedupe on `videoId`, plus a stable machine-readable exit-code map.
5. Optional online confirmation (oEmbed / Innertube) behind a `--online` flag —
   explicitly out of scope for phase 1.

---
