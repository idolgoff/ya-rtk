//! Build-only `ya make` filter (Stage 6).
//!
//! Collapses `[PB]` / `Ok [n/m]` / PEERDIR / hot-settings progress noise.
//! Keeps real `Error[` / `Warn[` (non-spam), compile-fail signals, pytest short
//! ERROR summary, and final make/outcome lines.
//!
//! Production uses `run_streamed` + [`YaBuildStreamFilter`] (Stage 9). The same
//! handler powers [`filter_ya_build`] for unit-test oracle parity. Tee hints come
//! from the runner only — this filter does **not** call `force_tee_hint`.

use crate::core::stream::{LineHandler, LineStreamFilter, StreamFilter};
use crate::core::truncate::CAP_ERRORS;
use crate::core::utils::strip_ansi;

/// True when argv is `ya make` without test flags (`-t` / `-tt*` / `--test`).
pub fn is_build_mode(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("make"))
        && !crate::cmds::yandex::envelope::is_test_mode(args)
}

/// Pure filter for build-mode `ya make` dumps (unit-test oracle; mirrors stream handler).
/// Production runs [`build_stream_filter`] via `run_streamed`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn filter_ya_build(raw: &str) -> String {
    let clean = strip_ansi(raw);
    if clean.trim().is_empty() {
        return String::new();
    }

    let mut filter = build_stream_filter();
    let mut kept = String::new();
    for line in clean.lines() {
        if let Some(chunk) = filter.feed_line(line) {
            kept.push_str(&chunk);
        }
    }
    kept.push_str(&filter.flush());
    if let Some(summary) = filter.on_exit(0, &clean) {
        kept.push_str(&summary);
    }
    if !kept.is_empty() && !kept.ends_with('\n') {
        kept.push('\n');
    }
    kept
}

/// Line handler shared by the buffered oracle and `run_streamed` path.
#[derive(Debug, Default)]
pub struct YaBuildHandler {
    pb_suppressed: usize,
    ok_suppressed: usize,
    hot_settings: usize,
    peerdir_warns: usize,
    error_lines: usize,
    seen_connection_refused: bool,
}

impl LineHandler for YaBuildHandler {
    fn should_skip(&mut self, line: &str) -> bool {
        let t = line.trim_start();
        if t.is_empty() {
            return true;
        }

        // Categorize known spam (even if we would drop anyway)
        if t.starts_with("------- [PB]") || t.starts_with("-------[PB]") {
            self.pb_suppressed += 1;
            return true;
        }
        if t.contains(".proto:") && t.contains("warning: Import") {
            self.pb_suppressed += 1;
            return true;
        }
        if t == "Ok" || t.starts_with("Ok [") {
            self.ok_suppressed += 1;
            return true;
        }
        if t.contains("Unexpected getting uninitialized hot settings") {
            self.hot_settings += 1;
            return true;
        }
        if t.contains("PEERDIR") || t.contains("UserWarning:") || t.contains("warnings.warn(")
        {
            self.peerdir_warns += 1;
            return true;
        }
        if t.contains("Connection refused") || t.contains("OperationalError") {
            self.seen_connection_refused = true;
        }

        // Allowlist: real signal only
        if is_build_signal(t) {
            if t.starts_with("ERROR ") {
                self.error_lines += 1;
                if self.error_lines > CAP_ERRORS {
                    return true;
                }
            }
            return false;
        }

        true
    }

    fn format_summary(&self, _exit_code: i32, _raw: &str) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if self.pb_suppressed > 0 {
            parts.push(format!(
                "… {}× [PB]/proto import noise suppressed",
                self.pb_suppressed
            ));
        }
        if self.ok_suppressed > 0 {
            parts.push(format!(
                "… {}× Ok progress suppressed",
                self.ok_suppressed
            ));
        }
        if self.hot_settings > 0 {
            parts.push(format!(
                "… {}× uninitialized hot settings warnings suppressed",
                self.hot_settings
            ));
        }
        if self.peerdir_warns > 0 {
            parts.push(format!(
                "… {}× UserWarning/PEERDIR suppressed",
                self.peerdir_warns
            ));
        }
        if self.seen_connection_refused {
            parts.push(
                "… repeated OperationalError: Connection refused (bodies in tee)".into(),
            );
        }
        if self.error_lines > CAP_ERRORS {
            parts.push(format!(
                "… +{} more ERROR lines",
                self.error_lines - CAP_ERRORS
            ));
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join("\n") + "\n")
    }
}

fn is_build_signal(t: &str) -> bool {
    // Ya / make diagnostics
    if t.starts_with("Error[") || t.starts_with("Error:") {
        return true;
    }
    if t.starts_with("Warn[") && !t.contains("BUILD_ONLY_IF") {
        return true;
    }
    if t.starts_with("make:") {
        return true;
    }
    if t.starts_with("------- [GO]") && t.contains("FAILED") {
        return true;
    }
    if t.contains("[TM]") || t.starts_with("------- [TM]") {
        return true;
    }

    // Pure compile-fail (no pytest framing)
    if t.contains("could not import") {
        return true;
    }
    if t.contains("BUILD ERRORS") || t.contains("BUILD ERROR") {
        return true;
    }
    // `file.go:17:5: …` compiler diagnostics
    if looks_like_go_compile_diag(t) {
        return true;
    }

    // Pytest compact signal (bodies dropped)
    if t.starts_with("ERROR ") || t.starts_with("FAILED ") {
        return true;
    }
    if t.starts_with('=')
        && (t.contains("ERROR")
            || t.contains("short test summary")
            || t.contains("errors in")
            || t.contains("failed in")
            || t.contains("passed in")
            || t.contains("test session"))
    {
        return true;
    }
    if t.contains(" errors in ") || t.contains(" failed in ") {
        return true;
    }
    // Final totals that aren't progress Ok
    if t.starts_with("Total ") || t.starts_with("Failed") {
        return true;
    }
    false
}

fn looks_like_go_compile_diag(t: &str) -> bool {
    // Prefer source paths over tool argv noise (`…/compile -o …`).
    if t.contains("/compile ") || t.starts_with("command (pid:") {
        return false;
    }
    let Some(colon) = t.find(".go:") else {
        return false;
    };
    let after = &t[colon + 4..];
    after
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
}

fn truncate_kept(line: &str) -> String {
    const MAX: usize = 240;
    let t = line.trim_end();
    if t.chars().count() <= MAX {
        return t.to_string();
    }
    let mut s: String = t.chars().take(MAX.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// Stream filter that truncates kept lines (LineStreamFilter emits raw).
pub struct YaBuildStreamFilter {
    inner: LineStreamFilter<YaBuildHandler>,
}

impl YaBuildStreamFilter {
    pub fn new() -> Self {
        Self {
            inner: LineStreamFilter::new(YaBuildHandler::default()),
        }
    }
}

impl Default for YaBuildStreamFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamFilter for YaBuildStreamFilter {
    fn feed_line(&mut self, line: &str) -> Option<String> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner
                .feed_line(line)
                .map(|emitted| truncate_kept(emitted.trim_end()) + "\n")
        })) {
            Ok(chunk) => chunk,
            Err(_) => {
                eprintln!("rtk: ya filter warning: panic in ya_build stream; passing line through");
                Some(format!("{}\n", line))
            }
        }
    }

    fn flush(&mut self) -> String {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.inner.flush())) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("rtk: ya filter warning: panic in ya_build flush");
                String::new()
            }
        }
    }

    fn on_exit(&mut self, exit_code: i32, raw: &str) -> Option<String> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner.on_exit(exit_code, raw)
        })) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("rtk: ya filter warning: panic in ya_build on_exit");
                None
            }
        }
    }
}

/// Stream filter entry for `run_streamed` (also powers [`filter_ya_build`]).
pub fn build_stream_filter() -> YaBuildStreamFilter {
    YaBuildStreamFilter::new()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn error_line_count(out: &str) -> usize {
        out.lines()
            .filter(|l| l.trim_start().starts_with("ERROR "))
            .count()
    }

    #[test]
    fn build_mode_detect() {
        assert!(is_build_mode(
            &["make".into(), "python".into(), "-r".into()]
        ));
        assert!(!is_build_mode(&["make".into(), "-t".into(), "path".into()]));
        assert!(!is_build_mode(&["make".into(), "-ttX".into()]));
        assert!(!is_build_mode(&["test".into(), "-r".into()]));
        assert!(!is_build_mode(&["tool".into(), "dump".into()]));
    }

    /// S6-T2: G7 ≥60%, keep real errors / outcome.
    #[test]
    fn g7_build_proto_saves_and_keeps_errors() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_python_py_build_large_proto_raw.txt");
        let out = filter_ya_build(raw);
        assert!(
            out.contains("ERROR ") || out.contains("errors in"),
            "must keep failure signal\n{out}"
        );
        assert!(
            out.contains("make:") || out.contains("Error 1") || out.contains("26 errors"),
            "must keep final outcome\n{out}"
        );
        assert!(
            !out.contains("------- [PB]"),
            "[PB] spam must drop\n{out}"
        );
        assert!(
            !out.contains("Unexpected getting uninitialized hot settings"),
            "hot settings spam must drop\n{out}"
        );
        // No in-filter tee hint — runner `with_tee` owns that.
        assert!(
            !out.contains("[full output:") && !out.contains("[see remaining:"),
            "filter must not embed tee hints\n{out}"
        );
        assert!(
            savings(raw, &out) >= 60.0,
            "savings {:.1}%\n{out}",
            savings(raw, &out)
        );
    }

    /// CAP_ERRORS=20: G7 overflows — keep exactly the cap, summarize the rest.
    #[test]
    fn g7_caps_error_lines_with_overflow_summary() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_python_py_build_large_proto_raw.txt");
        let raw_errors = raw
            .lines()
            .filter(|l| l.trim_start().starts_with("ERROR "))
            .count();
        assert!(raw_errors > CAP_ERRORS, "fixture must exercise the cap");
        let out = filter_ya_build(raw);
        assert_eq!(
            error_line_count(&out),
            CAP_ERRORS,
            "kept ERROR lines must equal CAP_ERRORS\n{out}"
        );
        assert!(
            out.contains("more ERROR lines"),
            "overflow must be summarized, not silent\n{out}"
        );
        assert!(out.contains("26 errors") || out.contains("errors in"));
    }

    /// Pure compile-fail build (no pytest framing) — `make_unk_fail_raw.txt`.
    #[test]
    fn compile_fail_keeps_go_failed_and_import_error() {
        let raw = include_str!("../../../tests/fixtures/ya/make_unk_fail_raw.txt");
        assert!(is_build_mode(&["make".into(), "-r".into()]));
        let out = filter_ya_build(raw);
        assert!(
            out.contains("[GO]") && out.contains("FAILED"),
            "must keep [GO] FAILED\n{out}"
        );
        assert!(
            out.contains("could not import") || out.contains("main.go:"),
            "must keep compile diagnostic\n{out}"
        );
        assert!(out.contains("Failed"), "must keep Failed outcome\n{out}");
        assert!(
            !out.contains("command (pid:"),
            "tool argv noise should drop\n{out}"
        );
        assert!(
            savings(raw, &out) >= 40.0,
            "compile-fail should still compress (got {:.1}%)\n{out}",
            savings(raw, &out)
        );
    }

    /// S6-T5: locked shape (no insta).
    #[test]
    fn g7_locked_shape() {
        let raw =
            include_str!("../../../tests/fixtures/ya/make_python_py_build_large_proto_raw.txt");
        let out = filter_ya_build(raw);
        assert!(
            out.contains("proto import noise suppressed") || out.contains("[PB]"),
            "PB suppression summary\n{out}"
        );
        assert!(out.contains("ERROR yandex_pay_plus") || out.contains("ERROR "));
        assert!(out.contains("26 errors") || out.contains("errors in"));
        assert!(out.contains("make:"));
        assert!(!out
            .lines()
            .any(|l| l.trim_start().starts_with("------- [PB]")));
    }
}
