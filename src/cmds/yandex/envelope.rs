//! Outer Arcadia envelope filter for `ya make -t*` / `ya test` output.
//!
//! Dispatches by [`detect_inner_runner`](crate::cmds::yandex::detect::detect_inner_runner):
//! py3test → pytest reuse; go_test → go-under-ya compact; otherwise generic fail.
//!
//! Production long suites use [`YaTestStreamFilter`] via `run_streamed` (Stage 9):
//! emit framing + compact `[fail]` blocks live (fat Expected/progress dropped),
//! so multi-MB assertion dumps are never buffered. Full-raw recovery is the
//! runner tee (`with_tee("ya")`); overflow lists use [`fail_overflow_tee_hint`].

use crate::cmds::yandex::adapters::generic_fail::{
    compact_fail_block, fail_overflow_tee_hint, is_fail_block_boundary, is_fat_drop_line,
    split_fail_sections, MAX_FAIL_BLOCKS,
};
use crate::cmds::yandex::adapters::go_test::filter_go_test;
use crate::cmds::yandex::adapters::py3test::filter_py3test;
use crate::cmds::yandex::detect::{detect_inner_runner, InnerRunner};
use crate::cmds::yandex::framing::keep_framing_line;
use crate::core::guard::never_worse;
use crate::core::stream::StreamFilter;
use crate::core::utils::strip_ansi;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;

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
///
/// Does **not** embed `force_tee_hint` for full raw — runner `with_tee("ya")`
/// owns that. Capped fail lists still get [`fail_overflow_tee_hint`].
pub fn filter_ya_envelope(raw: &str) -> String {
    let clean = strip_ansi(raw);
    if clean.trim().is_empty() {
        return String::new();
    }

    let (body, _truncated) = match detect_inner_runner(&clean) {
        InnerRunner::Py3test => {
            if let Some(result) = filter_py3test(&clean) {
                result
            } else {
                filter_generic(&clean)
            }
        }
        InnerRunner::GoTest => filter_go_test(&clean),
        // JS adapter optional (Stage 5) — no fixtures yet; generic fail.
        InnerRunner::JestVitest | InnerRunner::Unknown => filter_generic(&clean),
    };

    body
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
        let headlines: Vec<&str> = blocks
            .iter()
            .filter_map(|b| b.first().map(|l| l.trim_end()))
            .collect();
        if let Some(hint) = fail_overflow_tee_hint(&headlines, take_n) {
            out.push(hint);
        }
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

/// Streaming filter for long `ya` test suites (Stage 9).
///
/// - Drops fat Expected/progress lines immediately (never buffered).
/// - Buffers preamble until the first `[fail]`, then emits framing + compact
///   fail blocks live as each block closes.
/// - No-fail suites: filter at [`on_exit`] with [`never_worse`] against full raw.
/// - Full-raw recovery: runner tee only (no in-filter `force_tee_hint`).
pub struct YaTestStreamFilter {
    /// Lines before the first `[fail]` (fat already dropped).
    preamble: Vec<String>,
    seen_fail: bool,
    preamble_flushed: bool,
    current_block: Vec<String>,
    fails_emitted: usize,
    headlines: Vec<String>,
    seen_logsdir: HashSet<String>,
    #[allow(dead_code)] // retained for future summary / metrics
    truncated: bool,
    dropped_fat: bool,
    /// Accumulated emitted text (for never_worse / tests).
    emitted: String,
}

impl YaTestStreamFilter {
    pub fn new() -> Self {
        Self {
            preamble: Vec::new(),
            seen_fail: false,
            preamble_flushed: false,
            current_block: Vec::new(),
            fails_emitted: 0,
            headlines: Vec::new(),
            seen_logsdir: HashSet::new(),
            truncated: false,
            dropped_fat: false,
            emitted: String::new(),
        }
    }

    fn note_emit(&mut self, chunk: &str) {
        self.emitted.push_str(chunk);
    }

    fn flush_preamble_framing(&mut self) -> String {
        if self.preamble_flushed {
            return String::new();
        }
        self.preamble_flushed = true;
        let mut out = String::new();
        for line in &self.preamble {
            if keep_framing_line(line, true) {
                out.push_str(line.trim_end());
                out.push('\n');
            } else {
                self.truncated = true;
            }
        }
        self.preamble.clear();
        out
    }

    fn close_fail_block(&mut self) -> Option<String> {
        if self.current_block.is_empty() {
            return None;
        }
        let block = std::mem::take(&mut self.current_block);
        if let Some(h) = block.first() {
            self.headlines.push(h.trim_end().to_string());
        }
        if self.fails_emitted >= MAX_FAIL_BLOCKS {
            self.truncated = true;
            return None;
        }
        let refs: Vec<&str> = block.iter().map(String::as_str).collect();
        let (compact, block_trunc) = compact_fail_block(&refs);
        self.truncated |= block_trunc;
        let mut out = String::new();
        for line in compact {
            let t = line.trim_start();
            if t.starts_with("Logsdir:") && !self.seen_logsdir.insert(t.to_string()) {
                self.truncated = true;
                continue;
            }
            out.push_str(&line);
            out.push('\n');
        }
        self.fails_emitted += 1;
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn overflow_tail(&mut self) -> String {
        let total = self.headlines.len();
        if total <= MAX_FAIL_BLOCKS {
            return String::new();
        }
        self.truncated = true;
        let mut out = format!("… +{} more failing tests\n", total - MAX_FAIL_BLOCKS);
        let refs: Vec<&str> = self.headlines.iter().map(String::as_str).collect();
        if let Some(hint) = fail_overflow_tee_hint(&refs, MAX_FAIL_BLOCKS) {
            out.push_str(&hint);
            out.push('\n');
        }
        out
    }

    #[cfg(test)]
    fn did_drop_fat(&self) -> bool {
        self.dropped_fat
    }

    #[cfg(test)]
    fn emitted_so_far(&self) -> &str {
        &self.emitted
    }
}

impl Default for YaTestStreamFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamFilter for YaTestStreamFilter {
    fn feed_line(&mut self, line: &str) -> Option<String> {
        if is_fat_drop_line(line) {
            self.truncated = true;
            self.dropped_fat = true;
            return None;
        }

        let trimmed = line.trim_start();

        if !self.seen_fail {
            if trimmed.starts_with("[fail]") {
                self.seen_fail = true;
                let out = self.flush_preamble_framing();
                self.current_block.push(line.to_string());
                if !out.is_empty() {
                    self.note_emit(&out);
                    return Some(out);
                }
                return None;
            }
            self.preamble.push(line.to_string());
            return None;
        }

        // Fail / post-fail streaming
        if trimmed.starts_with("[fail]") {
            let mut out = String::new();
            if let Some(closed) = self.close_fail_block() {
                out.push_str(&closed);
            }
            self.current_block.push(line.to_string());
            if out.is_empty() {
                return None;
            }
            self.note_emit(&out);
            return Some(out);
        }

        if !self.current_block.is_empty() {
            if is_fail_block_boundary(line) {
                let mut out = String::new();
                if let Some(closed) = self.close_fail_block() {
                    out.push_str(&closed);
                }
                if keep_framing_line(line, true) {
                    out.push_str(line.trim_end());
                    out.push('\n');
                } else {
                    self.truncated = true;
                }
                if out.is_empty() {
                    return None;
                }
                self.note_emit(&out);
                return Some(out);
            }
            self.current_block.push(line.to_string());
            return None;
        }

        // Postamble
        if keep_framing_line(line, true) {
            let chunk = format!("{}\n", line.trim_end());
            self.note_emit(&chunk);
            return Some(chunk);
        }
        self.truncated = true;
        None
    }

    fn flush(&mut self) -> String {
        if !self.seen_fail {
            // Defer no-fail filtering to on_exit (needs full raw for never_worse).
            return String::new();
        }
        let mut out = String::new();
        if let Some(closed) = self.close_fail_block() {
            out.push_str(&closed);
        }
        out.push_str(&self.overflow_tail());
        if !out.is_empty() {
            self.note_emit(&out);
        }
        out
    }

    fn on_exit(&mut self, _exit_code: i32, raw: &str) -> Option<String> {
        if self.seen_fail {
            // Live path already printed; never_worse would not un-print.
            // Compact fails are always ≪ raw, so no action.
            return None;
        }
        // No-fail / unknown: filter full raw, then never_worse.
        let filtered = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            filter_ya_envelope(raw)
        })) {
            Ok(s) => s,
            Err(_) => {
                eprintln!(
                    "rtk: ya filter warning: panic in filter_ya_envelope; showing raw output"
                );
                raw.to_string()
            }
        };
        let shown = never_worse(raw, &filtered).to_string();
        self.note_emit(&shown);
        Some(shown)
    }
}

/// Stream filter entry for `run_streamed` (test-mode `ya make -t*` / `ya test`).
pub fn test_stream_filter() -> YaTestStreamFilter {
    YaTestStreamFilter::new()
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

    /// S4-T4: `-ttX` verbose dumps still get fail-block extraction (≥60%).
    #[test]
    fn g10_ttx_fail_extracts_fails() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_ttX_py_fail_logsdir_chunk_raw.txt");
        assert!(
            is_test_mode(&["make".into(), "-ttX".into()]),
            "-ttX must select test-mode filter"
        );
        let out = filter_ya_envelope(raw);
        assert!(
            out.contains("test_response") || out.contains("TestRenderOrder"),
            "must keep failed node id\n{out}"
        );
        assert!(out.contains("[FAIL]") || out.contains("failed") || out.contains("[fail]"));
        assert!(out.contains("Logsdir:"));
        assert_min_savings(raw, &out, 60.0);
    }

    #[test]
    fn g1_locked_shape() {
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

    /// Locked shape asserts (no insta — same pattern as Stage 2).
    #[test]
    fn g5_locked_shape() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt");
        let out = filter_ya_envelope(raw);
        assert!(out.contains("<go_test>") || out.contains("chunk"));
        assert_eq!(
            out.lines().filter(|l| l.starts_with("[fail]")).count(),
            2
        );
        assert!(out.contains("------ FAIL") || out.contains("FAIL"));
    }

    /// S5-T4: G6 via full envelope dispatch (detector → go adapter).
    #[test]
    fn g6_go_other_via_envelope() {
        let raw = include_str!("../../../tests/fixtures/ya/make_tt_go_other_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::GoTest);
        let out = filter_ya_envelope(raw);
        assert!(
            out.contains("Error[-WSyntax]") || out.contains("unexpected command"),
            "out={out}"
        );
        assert!(!out.contains("BUILD_ONLY_IF"));
        assert_min_savings(raw, &out, 60.0);
    }

    fn filter_via_stream(raw: &str) -> String {
        let mut filter = test_stream_filter();
        let mut out = String::new();
        for line in raw.lines() {
            if let Some(chunk) = filter.feed_line(line) {
                out.push_str(&chunk);
            }
        }
        out.push_str(&filter.flush());
        if let Some(tail) = filter.on_exit(0, raw) {
            out.push_str(&tail);
        }
        out
    }

    /// S9-T1: stream path on G1 keeps signal + Logsdir and still saves ≥60%.
    #[test]
    fn stream_filter_g1_keeps_logsdir_and_saves() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
        let out = filter_via_stream(raw);
        assert!(out.contains("test_returned_fiscal_contact"), "node id\n{out}");
        assert!(out.contains("Logsdir:"), "Logsdir\n{out}");
        assert!(out.contains("Log:"), "Log:\n{out}");
        assert_min_savings(raw, &out, 60.0);
    }

    /// S9-T1: live emit — first fail block appears before EOF (not only at flush).
    #[test]
    fn stream_filter_emits_fail_block_before_flush() {
        let mut filter = test_stream_filter();
        let _ = filter.feed_line("------- [TM] {default-linux-x86_64, release} suite/py3test");
        assert!(
            filter.emitted_so_far().is_empty(),
            "preamble must wait for first [fail]"
        );
        let _ = filter.feed_line("[fail] mod::test_a [default-linux-x86_64-debug] (0.1s)");
        let _ = filter.feed_line("path.py:1: in test_a");
        let _ = filter.feed_line("E   AssertionError");
        let _ = filter.feed_line("Logsdir: /tmp/out");
        // Closing boundary emits the compact block live
        let chunk = filter.feed_line("------ FAIL: 1 - FAIL suite");
        assert!(
            chunk.is_some(),
            "fail block must emit on boundary before flush"
        );
        let live = chunk.unwrap();
        assert!(live.contains("[fail] mod::test_a") || live.contains("test_a"));
        assert!(live.contains("Logsdir:"));
        assert!(
            filter.emitted_so_far().contains("Logsdir:"),
            "live path must have emitted before flush"
        );
    }

    /// S9-T1: fat Expected dumps never enter the fail-block buffer.
    #[test]
    fn stream_filter_drops_fat_expected_live() {
        let mut filter = test_stream_filter();
        let lines = [
            "[fail] mod::test_x [default-darwin-arm64-debug] (0.1s)",
            "path.py:10: in test_x",
            "E   AssertionError: boom",
            "E   Expected: <{'huge': 'blob'}>",
            "E        but: was <{'other': 1}>",
            "Log: /tmp/test.log",
            "Logsdir: /tmp/out",
            "------ FAIL: 1 - FAIL suite",
        ];
        let mut out = String::new();
        for line in lines {
            if let Some(chunk) = filter.feed_line(line) {
                out.push_str(&chunk);
            }
        }
        out.push_str(&filter.flush());
        assert!(filter.did_drop_fat(), "Expected/but must be dropped");
        assert!(out.contains("Logsdir:"));
        assert!(out.contains("test_x") || out.contains("[fail]"));
        assert!(!out.contains("Expected:"));
        assert!(!out.contains("[full output:"), "no in-filter full-raw tee");
    }

    /// S9-T2: production stream path retains Logsdir: / Log: on fail goldens.
    #[test]
    fn logsdir_never_stripped_on_fail_goldens() {
        const FIXTURES: &[&str] = &[
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt"),
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_raw.txt"),
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_slice_raw.txt"),
            include_str!("../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt"),
            include_str!("../../../tests/fixtures/ya/make_ttX_py_fail_logsdir_chunk_raw.txt"),
        ];
        for raw in FIXTURES {
            assert!(
                raw.contains("Logsdir:"),
                "fixture must contain Logsdir for this audit"
            );
            let out = filter_via_stream(raw);
            assert!(
                out.contains("Logsdir:"),
                "Logsdir must survive stream filter\n--- out (truncated) ---\n{}",
                out.chars().take(800).collect::<String>()
            );
            if raw.lines().any(|l| l.trim_start().starts_with("Log:")) {
                assert!(
                    out.contains("Log:"),
                    "Log: paths must survive stream filter when present in raw"
                );
            }
        }
    }

    /// S9-T3: CAP_ERRORS overflow emits “… +N more” (tee hint when tee enabled).
    #[test]
    fn capped_fail_list_emits_overflow_summary() {
        let mut raw = String::from("------- [TM] {default-linux-x86_64, release} suite/py3test\n");
        for i in 0..(MAX_FAIL_BLOCKS + 5) {
            raw.push_str(&format!(
                "[fail] mod::test_{i} [default-linux-x86_64-debug] (0.01s)\n"
            ));
            raw.push_str("path.py:1: in test\n");
            raw.push_str("E   AssertionError\n");
            raw.push_str("Log: /tmp/t.log\n");
            raw.push_str("Logsdir: /tmp/out\n");
        }
        raw.push_str("Total 1 suite: 25 FAIL\n");
        let out = filter_via_stream(&raw);
        assert!(
            out.contains("more failing tests"),
            "overflow must be explicit\n{out}"
        );
        assert!(out.contains("Logsdir:"));
        let fail_headlines = out
            .lines()
            .filter(|l| l.trim_start().starts_with("[fail]"))
            .count();
        assert_eq!(
            fail_headlines, MAX_FAIL_BLOCKS,
            "stream must keep exactly CAP fails\n{out}"
        );
        // If tee produced a hint, offset must be CAP+1 (full list contract).
        if let Some(hint_line) = out.lines().find(|l| l.contains("[see remaining:")) {
            assert!(
                hint_line.contains(&format!("tail -n +{}", MAX_FAIL_BLOCKS + 1)),
                "overflow tee offset must be shown+1 on full list: {hint_line}"
            );
        }
    }

    /// S9-T4: linux platform tags in fail headlines still extract / keep node ids.
    #[test]
    fn linux_platform_fail_headline_retained() {
        let raw = "\
------- [TM] {default-linux-x86_64, release} pay/lib/py3test
[fail] api.test_order.py::test_sku [default-linux-x86_64-debug] (0.14s)
api/test_order.py:10: in test_sku
E   AssertionError: boom
Log: /tmp/test.log
Logsdir: /tmp/test-results/py3test
Total 1 suite: 1 FAIL
";
        let out = filter_via_stream(raw);
        assert!(
            out.contains("test_sku"),
            "linux-tagged node id must remain\n{out}"
        );
        assert!(out.contains("Logsdir:"));
    }

    /// S9-T5: filter-only perf smoke on large fixture (no live `ya`).
    #[test]
    fn large_fixture_filter_perf_smoke() {
        use std::time::Instant;
        let raw =
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_slice_raw.txt");
        let start = Instant::now();
        const ITERS: u32 = 20;
        for _ in 0..ITERS {
            let out = filter_via_stream(raw);
            assert!(out.contains("Logsdir:") || out.contains("[fail]") || out.contains("failed"));
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 2_000,
            "{ITERS}× large stream filter took {elapsed:?} (budget 2s)"
        );
    }

    /// S9-T5: stream filter on large fixture also within budget.
    #[test]
    fn large_fixture_stream_filter_perf_smoke() {
        use std::time::Instant;
        let raw =
            include_str!("../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_slice_raw.txt");
        let start = Instant::now();
        let out = filter_via_stream(raw);
        let elapsed = start.elapsed();
        assert!(out.contains("Logsdir:") || out.contains("[fail]") || out.contains("failed"));
        assert!(
            elapsed.as_millis() < 500,
            "stream filter on large fixture took {elapsed:?}"
        );
    }

    /// S9-T6: NUL / binary noise / empty never panic.
    #[test]
    fn fuzz_binary_noise_and_nul_never_panic() {
        let mut s = String::from("------- [TM] noise/py3test\n");
        s.push('\0');
        s.push_str("[fail] t::x [default-linux-arm64-debug] (0.1s)\n");
        s.push_str(&String::from_utf8_lossy(&[0xff, 0xfe, 0x00, 0x01]));
        s.push('\n');
        s.push_str("Logsdir: /tmp/out\n");
        let _ = filter_ya_envelope(&s);
        let _ = filter_via_stream(&s);
        let _ = filter_ya_envelope("\0\0\0");
        let _ = filter_via_stream("");
    }

    /// S9-T6: truncated mid-fail still keeps headline / partial signal.
    #[test]
    fn fuzz_truncated_mid_fail_keeps_headline() {
        let raw = "\
[fail] mod::test_partial [default-darwin-arm64-debug] (0.1s)
path.py:10: in test_partial
E   AssertionError: cut off mid
";
        let out = filter_via_stream(raw);
        assert!(
            out.contains("test_partial") || out.contains("[fail]"),
            "truncated fail must not lose node id\n{out}"
        );
        assert!(!out.contains("Logsdir:") || raw.contains("Logsdir:"));
    }

    /// S9-T6: mid-fail cut after Logsdir still retains it.
    #[test]
    fn fuzz_truncated_after_logsdir_keeps_path() {
        let raw = "\
[fail] mod::test_y [default-linux-x86_64-debug] (0.1s)
path.py:1: in test_y
E   AssertionError
Logsdir: /tmp/results/py3test
";
        let out = filter_via_stream(raw);
        assert!(out.contains("Logsdir:"));
        assert!(out.contains("test_y") || out.contains("[fail]"));
    }

    /// Stream path must not embed full-raw tee hints (runner owns that).
    #[test]
    fn stream_filter_no_double_full_tee_hint() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
        let out = filter_via_stream(raw);
        assert!(
            !out.contains("[full output:"),
            "in-filter must not force_tee_hint full raw\n{out}"
        );
    }
}
