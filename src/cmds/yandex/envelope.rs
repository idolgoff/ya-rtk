//! Outer Arcadia envelope filter for `ya make -t*` / `ya test` output.
//!
//! Dispatches by [`detect_inner_runner`](crate::cmds::yandex::detect::detect_inner_runner):
//! py3test → pytest reuse; otherwise generic fail compact (Stage 2).

use crate::cmds::yandex::adapters::generic_fail::{
    compact_fail_block, split_fail_sections, MAX_FAIL_BLOCKS,
};
use crate::cmds::yandex::adapters::py3test::filter_py3test;
use crate::cmds::yandex::detect::{detect_inner_runner, InnerRunner};
use crate::cmds::yandex::framing::keep_framing_line;
use crate::core::tee::force_tee_hint;
use crate::core::utils::strip_ansi;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref TEST_FLAG_RE: Regex = Regex::new(r"^-t+X?$").unwrap();
}

/// True when argv should use the test-mode envelope filter.
pub fn is_test_mode(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("test") => true,
        Some("make") => args.iter().any(|a| is_ya_test_flag(a)),
        _ => false,
    }
}

fn is_ya_test_flag(arg: &str) -> bool {
    arg == "--test" || TEST_FLAG_RE.is_match(arg)
}

/// Filter combined ya test-mode stdout/stderr text.
pub fn filter_ya_envelope(raw: &str) -> String {
    let clean = strip_ansi(raw);
    if clean.trim().is_empty() {
        return String::new();
    }

    let (body, truncated) = match detect_inner_runner(&clean) {
        InnerRunner::Py3test => {
            if let Some(result) = filter_py3test(&clean) {
                result
            } else {
                filter_generic(&clean)
            }
        }
        // Go / JS adapters land in later stages — generic fail until then.
        InnerRunner::GoTest | InnerRunner::JestVitest | InnerRunner::Unknown => {
            filter_generic(&clean)
        }
    };

    finalize(raw, body, truncated)
}

fn finalize(raw: &str, body: String, truncated: bool) -> String {
    let mut out = body;
    if truncated {
        if let Some(hint) = force_tee_hint(raw, "ya") {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&hint);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn filter_generic(raw: &str) -> (String, bool) {
    let has_fail = raw.lines().any(|l| l.trim_start().starts_with("[fail]"));
    if has_fail {
        filter_with_fails(raw)
    } else {
        filter_without_fails(raw)
    }
}

fn filter_with_fails(raw: &str) -> (String, bool) {
    let (preamble, blocks, postamble) = split_fail_sections(raw);
    let mut out: Vec<String> = Vec::new();
    let mut truncated = false;

    for line in preamble {
        if keep_framing_line(line, true) {
            out.push(line.trim_end().to_string());
        }
    }

    let total = blocks.len();
    let take_n = total.min(MAX_FAIL_BLOCKS);
    for block in blocks.iter().take(take_n) {
        let refs = block.to_vec();
        let (compact, block_trunc) = compact_fail_block(&refs);
        truncated |= block_trunc;
        out.extend(compact);
    }
    if total > take_n {
        truncated = true;
        out.push(format!("… +{} more failing tests", total - take_n));
    }

    for line in postamble {
        if keep_framing_line(line, true) {
            out.push(line.trim_end().to_string());
        }
    }

    // Always truncated when we compact fail bodies (Expected/but dropped)
    if take_n > 0 {
        truncated = true;
    }

    (join_lines(out), truncated)
}

fn filter_without_fails(raw: &str) -> (String, bool) {
    let mut out: Vec<String> = Vec::new();
    let mut warn_kept = 0usize;
    let mut hot_settings = 0usize;
    let mut truncated = false;
    const MAX_WARNS: usize = 5;

    for line in raw.lines() {
        let t = line.trim_start();

        if t.contains("PEERDIR") || t.contains("[PB]") || t.starts_with("Ok [") {
            truncated = true;
            continue;
        }

        if t.contains("Unexpected getting uninitialized hot settings") {
            hot_settings += 1;
            truncated = true;
            continue;
        }

        if t.starts_with("Warn[") || t.contains("UserWarning:") {
            if warn_kept < MAX_WARNS {
                out.push(truncate_kept(line));
                warn_kept += 1;
            } else {
                truncated = true;
            }
            continue;
        }

        // Long tool argv / compile lines — drop (tee has raw)
        if t.starts_with("command (pid:") {
            truncated = true;
            continue;
        }
        if t.contains("/compile ") && t.contains("returned non-zero") {
            truncated = true;
            continue;
        }

        if t.contains("could not import") {
            out.push(shorten_import_error(t));
            truncated = true;
            continue;
        }

        if t.starts_with("------- [GO]") && t.contains("FAILED") {
            out.push(shorten_go_failed(t));
            truncated = true;
            continue;
        }

        if t.starts_with("Number of suites skipped") {
            // "Number of suites skipped due to a failed build: 16"
            let n = t
                .rsplit(':')
                .next()
                .map(str::trim)
                .unwrap_or("?");
            out.push(format!("suites skipped (build fail): {n}"));
            truncated = true;
            continue;
        }

        if t.contains("DIDN'T RUN") || t.contains("didn't run") {
            // Covered by Failed + suites skipped
            truncated = true;
            continue;
        }

        if keep_framing_line(line, false) || keep_failure_signal(t) {
            out.push(truncate_kept(line));
            continue;
        }

        // Shell prompt / noise
        if t.starts_with('➜') || t.starts_with("COMMENTS") || t.is_empty() {
            continue;
        }

        truncated = true;
    }

    if hot_settings > 0 {
        out.push(format!(
            "… {}× uninitialized hot settings warnings suppressed",
            hot_settings
        ));
    }
    if warn_kept >= MAX_WARNS {
        out.push("… additional Warn/UserWarning lines suppressed".into());
    }

    (join_lines(out), truncated)
}

fn shorten_go_failed(t: &str) -> String {
    let target = t
        .split("$(B)/")
        .nth(1)
        .map(|s| s.split('{').next().unwrap_or(s))
        .map(|s| s.rsplit('/').next().unwrap_or(s))
        .unwrap_or("build");
    format!("------- [GO] FAILED {target}")
}

fn shorten_import_error(t: &str) -> String {
    // `…/main.go:17:5: could not import a.yandex-team.ru/…/schemas/uz (…)`
    let file = t
        .split(": could not import")
        .next()
        .and_then(|p| p.rsplit('/').next())
        .unwrap_or("file");
    let pkg = t
        .split("could not import ")
        .nth(1)
        .map(|s| s.split_whitespace().next().unwrap_or(s))
        .map(|s| {
            s.rsplit('/')
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|| "pkg".into());
    format!("{file}: could not import …/{pkg}")
}

fn keep_failure_signal(t: &str) -> bool {
    t.starts_with("Error[")
        || t.starts_with("Failed")
        || t.starts_with("FAILED")
        || t.contains("BUILD ERRORS")
        || t.contains("didn't run")
        || t.contains("DIDN'T RUN")
        || t.starts_with("Number of suites skipped")
        || t.starts_with("Test command err:")
}

fn truncate_kept(line: &str) -> String {
    const MAX: usize = 240;
    let t = line.trim_end();
    if t.chars().count() <= MAX {
        return t.to_string();
    }
    let mut s: String = t.chars().take(MAX - 1).collect();
    s.push('…');
    s
}

fn join_lines(lines: Vec<String>) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmds::yandex::detect::{detect_inner_runner, InnerRunner};

    fn count_tokens(s: &str) -> usize {
        s.split_whitespace().count()
    }

    fn savings(raw: &str, out: &str) -> f64 {
        let i = count_tokens(raw);
        let o = count_tokens(out);
        if i == 0 {
            return 0.0;
        }
        100.0 * (1.0 - o as f64 / i as f64)
    }

    fn assert_min_savings(raw: &str, out: &str, min: f64) {
        let s = savings(raw, out);
        assert!(
            s >= min,
            "expected ≥{min}% savings, got {s:.1}% ({} → {} tokens)\n--- out ---\n{out}",
            count_tokens(raw),
            count_tokens(out)
        );
    }

    #[test]
    fn test_mode_flags() {
        assert!(is_test_mode(&["make".into(), "-t".into(), "path".into()]));
        assert!(is_test_mode(&["make".into(), "-tt".into()]));
        assert!(is_test_mode(&["make".into(), "-ttX".into()]));
        assert!(is_test_mode(&["make".into(), "--test".into()]));
        assert!(is_test_mode(&["test".into(), "-r".into()]));
        assert!(!is_test_mode(&["make".into(), "python".into(), "-r".into()]));
        assert!(!is_test_mode(&["tool".into(), "dump".into()]));
    }

    #[test]
    fn empty_and_malformed_never_panic() {
        assert_eq!(filter_ya_envelope(""), "");
        let out = filter_ya_envelope("not valid ya output\nrandom text\n");
        // best-effort: may be empty or stripped noise — must not panic
        let _ = out;
    }

    #[test]
    fn g1_py_fail_keeps_fail_logsdir_and_saves() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(
            out.contains("test_returned_fiscal_contact"),
            "must keep node id"
        );
        assert!(out.contains("Logsdir:"), "must keep Logsdir");
        assert!(
            out.contains("[FAIL]") || out.contains("failed"),
            "failure signal\n{out}"
        );
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g5_go_fail_keeps_fail_logsdir_and_saves() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(out.contains("[fail]"));
        assert!(out.contains("Logsdir:"));
        assert!(out.contains("FAIL") || out.contains("chunk"));
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g2_large_slice_savings() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_slice_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(
            out.contains("[FAIL]") || out.contains("failed"),
            "expected failure signal\n{out}"
        );
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g3_large_chunk_keeps_logsdir() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(out.contains("[FAIL]") || out.contains("failed"));
        assert!(out.contains("Logsdir:"));
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g8_unk_fail_keeps_signal() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_unk_fail_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(
            out.contains("FAILED") || out.contains("Failed") || out.contains("BUILD ERRORS"),
            "out={out}"
        );
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g10_ttx_fail_extracts_fails() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_ttX_py_fail_logsdir_chunk_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(out.contains("[FAIL]") || out.contains("failed"));
        assert!(out.contains("Logsdir:"));
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g1_snapshot_shape() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
        let out = filter_ya_envelope(raw);
        // py3test adapter → pytest-style [FAIL] lines (3 node ids)
        assert!(out.contains("test_returned_fiscal_contact"));
        assert!(out.contains("test_returned_split_code"));
        assert!(out.contains("test_sku_version_excluded"));
        let mut logsdirs = 0;
        for line in out.lines() {
            if line.starts_with("Logsdir:") {
                logsdirs += 1;
            }
        }
        assert!(logsdirs >= 1, "at least one Logsdir retained");
    }

    #[test]
    fn g4_pass_mixed_savings() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_pass_or_mixed_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::Py3test);
        let out = filter_ya_envelope(raw);
        assert!(
            out.contains("[TM]") || out.contains("py3test") || out.contains("Total "),
            "expected framing\n{out}"
        );
        assert_min_savings(raw, &out, 40.0);
    }

    #[test]
    fn g5_snapshot_shape() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(out.contains("<go_test>") || out.contains("chunk"));
        assert_eq!(
            out.lines().filter(|l| l.starts_with("[fail]")).count(),
            2
        );
        assert!(out.contains("------ FAIL") || out.contains("FAIL"));
    }
}
