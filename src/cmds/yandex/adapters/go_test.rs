//! go_test adapter — compact ya-framed Go failures (and go-other build errors).
//!
//! Bare `go test -json` shapes do **not** appear under `ya`; we do not call
//! [`filter_go_test_json`](crate::cmds::go::go_cmd::filter_go_test_json). Instead:
//! fail blocks → [`compact_fail_block`] (S0-T5); no-fail → keep `Error[…]` signal,
//! drop Warn/BUILD_ONLY_IF + shell chrome.

use crate::cmds::yandex::adapters::generic_fail::{
    compact_fail_block, fail_overflow_tee_hint, split_fail_sections, MAX_FAIL_BLOCKS,
};
use crate::cmds::yandex::framing::keep_framing_line;

const MAX_LINE_CHARS: usize = 240;

/// Filter go_test-shaped `ya` output. Always returns a body (unlike py3test).
pub fn filter_go_test(raw: &str) -> (String, bool) {
    let has_fail = raw.lines().any(|l| l.trim_start().starts_with("[fail]"));
    if has_fail {
        filter_go_fails(raw)
    } else {
        filter_go_other(raw)
    }
}

fn filter_go_fails(raw: &str) -> (String, bool) {
    let (preamble, blocks, postamble) = split_fail_sections(raw);
    let mut out: Vec<String> = Vec::new();
    let mut truncated = false;
    // Dedupe identical Logsdir spam across blocks; Log: stays one-per-block (S0-T5).
    let mut seen_logsdir = std::collections::HashSet::new();

    for line in &preamble {
        if keep_framing_line(line, true) {
            out.push(line.trim_end().to_string());
        }
    }

    let total = blocks.len();
    let take_n = total.min(MAX_FAIL_BLOCKS);
    for block in blocks.iter().take(take_n) {
        let refs = block.as_slice();
        let (compact, block_trunc) = compact_fail_block(refs);
        truncated |= block_trunc;
        for line in compact {
            let t = line.trim_start();
            if t.starts_with("Logsdir:") && !seen_logsdir.insert(t.to_string()) {
                truncated = true;
                continue;
            }
            if t.starts_with("[fail]") {
                out.push(compact_fail_headline(&line));
                truncated = true;
                continue;
            }
            if t.starts_with("Log:") {
                out.push(compact_log_line(&line));
                truncated = true;
                continue;
            }
            out.push(line);
        }
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

    for line in &postamble {
        if keep_framing_line(line, true) {
            out.push(line.trim_end().to_string());
        }
    }

    if take_n > 0 {
        truncated = true;
    }

    (join_lines(out), truncated)
}

/// No `[fail]` blocks — typically ya.make / build diagnostics around gotest targets.
fn filter_go_other(raw: &str) -> (String, bool) {
    let mut out: Vec<String> = Vec::new();
    let mut truncated = false;
    let mut errors = 0usize;
    const MAX_ERRORS: usize = 20;

    for line in raw.lines() {
        let t = line.trim_start();

        if is_shell_chrome(t) {
            truncated = true;
            continue;
        }

        // BUILD_ONLY_IF / UserWarn spam — drop (tee recovers)
        if t.starts_with("Warn[") {
            truncated = true;
            continue;
        }

        if t.starts_with("Error[") || t.starts_with("Error:") {
            if errors < MAX_ERRORS {
                out.push(compact_ya_error_line(line));
                errors += 1;
                truncated = true; // paths shortened vs raw
            } else {
                truncated = true;
            }
            continue;
        }

        if keep_framing_line(line, false) {
            out.push(line.trim_end().to_string());
            continue;
        }

        if is_go_failure_signal(t) {
            out.push(truncate_line(line));
            continue;
        }

        if !t.is_empty() {
            truncated = true;
        }
    }

    (join_lines(out), truncated)
}

/// Keep Error tag + actionable message; drop long `$S/…` path prefixes.
fn compact_ya_error_line(line: &str) -> String {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix("Error[") else {
        return truncate_line(line);
    };
    let Some((tag, after)) = rest.split_once(']') else {
        return truncate_line(line);
    };
    let after = after.strip_prefix(':').unwrap_or(after).trim();

    if let Some(idx) = after.find("unexpected command ") {
        let msg = after[idx..]
            .split(". Only")
            .next()
            .unwrap_or(&after[idx..])
            .trim();
        return format!("Error[{tag}]: {msg}");
    }
    if let Some(file) = after.split("cannot find source file:").nth(1) {
        return format!(
            "Error[{tag}]: cannot find source file: {}",
            file.trim()
        );
    }
    if after.contains("dart field") || after.contains("DartValueError") {
        return format!("Error[{tag}]: dart field must not be empty");
    }

    truncate_line(&format!("Error[{tag}]: {after}"))
}

/// `[fail] name [platform] (0.1s)` → `[fail] name` (platform/time are noise).
fn compact_fail_headline(line: &str) -> String {
    let t = line.trim_end();
    if let Some(idx) = t.find(" [") {
        // Only strip trailing platform/time bracket, keep names that contain `[` rarely
        let head = t[..idx].trim_end();
        if head.starts_with("[fail]") {
            return head.to_string();
        }
    }
    truncate_line(line)
}

/// Keep `Log:` with basename only — full path lives under Logsdir / tee.
fn compact_log_line(line: &str) -> String {
    let t = line.trim_start();
    let Some(path) = t.strip_prefix("Log:") else {
        return truncate_line(line);
    };
    let path = path.trim();
    let base = path.rsplit('/').next().unwrap_or(path);
    format!("Log: …/{base}")
}

fn is_shell_chrome(t: &str) -> bool {
    t.starts_with('➜')
        || t.contains(" arc:(")
        // Absolute cwd echoes (macOS + Linux corpus / agents)
        || t.starts_with("/Users/")
        || t.starts_with("/home/")
        || t.starts_with("ya make")
        || t.ends_with("ya make -tt")
        || t.ends_with("ya make -t")
}

fn is_go_failure_signal(t: &str) -> bool {
    t.starts_with("Failed")
        || t == "Failed"
        || t.starts_with("FAIL\t")
        || t.starts_with("panic:")
        || (t.starts_with("--- FAIL:") || t.starts_with("=== FAIL"))
}

fn truncate_line(line: &str) -> String {
    let t = line.trim_end();
    if t.chars().count() <= MAX_LINE_CHARS {
        return t.to_string();
    }
    let mut s: String = t.chars().take(MAX_LINE_CHARS.saturating_sub(1)).collect();
    s.push('…');
    s
}

fn join_lines(lines: Vec<String>) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut body = lines.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body
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

    /// S5-T1: G5 go fail — headlines + Logsdir, ≥60%.
    #[test]
    fn g5_go_fail_keeps_headlines_logsdir_and_saves() {
        let raw =
            include_str!("../../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::GoTest);
        let (out, truncated) = filter_go_test(raw);
        assert!(truncated, "fail bodies should be truncated");
        assert!(
            out.contains("[fail] component::TestFeatures"),
            "headline\n{out}"
        );
        assert!(
            out.contains("TestFeatures/Загрузка_мерчанта")
                || out.contains("Загрузка_мерчанта"),
            "second fail\n{out}"
        );
        assert!(out.contains("Logsdir:"), "Logsdir\n{out}");
        assert!(out.contains("Log:"), "Log:\n{out}");
        assert!(
            out.contains("main_test.go:31") || out.contains("non-zero status"),
            "location/error head\n{out}"
        );
        assert!(
            savings(raw, &out) >= 60.0,
            "savings {:.1}%\n{out}",
            savings(raw, &out)
        );
    }

    /// S5-T4: G6 go other — errors kept, Warn/shell dropped, ≥60%.
    #[test]
    fn g6_go_other_keeps_errors_drops_warn_spam() {
        let raw = include_str!("../../../../tests/fixtures/ya/make_tt_go_other_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::GoTest);
        let (out, _) = filter_go_test(raw);
        assert!(
            out.contains("Error[-WSyntax]") || out.contains("unexpected command"),
            "must keep real errors\n{out}"
        );
        assert!(
            out.contains("GO_LIBRARY") || out.contains("root.go") || out.contains("SRCS"),
            "actionable path/command\n{out}"
        );
        assert!(
            !out.contains("BUILD_ONLY_IF"),
            "Warn BUILD_ONLY_IF should drop\n{out}"
        );
        assert!(!out.contains('➜'), "shell chrome should drop\n{out}");
        assert!(
            savings(raw, &out) >= 60.0,
            "savings {:.1}%\n{out}",
            savings(raw, &out)
        );
    }

    /// Extra: testify-style go fails under ya.
    #[test]
    fn tt_go_fail_keeps_test_names_and_logsdir() {
        let raw =
            include_str!("../../../../tests/fixtures/ya/make_tt_go_fail_logsdir_chunk_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::GoTest);
        let (out, _) = filter_go_test(raw);
        assert!(out.contains("TestSendFESJobBatchAction_V12"));
        assert!(out.contains("Logsdir:"));
        let fail_n = out
            .lines()
            .filter(|l| l.trim_start().starts_with("[fail]"))
            .count();
        let log_n = out
            .lines()
            .filter(|l| l.trim_start().starts_with("Log:"))
            .count();
        assert_eq!(log_n, fail_n, "S0-T5: one Log: per [fail] block\n{out}");
        assert!(savings(raw, &out) >= 60.0, "savings {:.1}%", savings(raw, &out));
    }

    /// Locked shape asserts (no insta in this project — same pattern as Stage 2).
    #[test]
    fn g5_locked_shape() {
        let raw =
            include_str!("../../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt");
        let (out, _) = filter_go_test(raw);
        assert!(out.contains("<go_test>") || out.contains("chunk"));
        assert_eq!(
            out.lines()
                .filter(|l| l.trim_start().starts_with("[fail]"))
                .count(),
            2
        );
        assert!(out.contains("------ FAIL") || out.contains("FAIL"));
        let logsdirs = out
            .lines()
            .filter(|l| l.trim_start().starts_with("Logsdir:"))
            .count();
        assert!(logsdirs >= 1);
        let log_n = out
            .lines()
            .filter(|l| l.trim_start().starts_with("Log:"))
            .count();
        assert_eq!(log_n, 2, "one Log: per fail block\n{out}");
    }
}
