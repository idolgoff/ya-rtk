//! Shared Arcadia envelope framing helpers (suite/chunk/totals identity lines).

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    /// Indented suite/test outcome: `84 - GOOD`, `2 - FAIL`, …
    static ref OUTCOME_RE: Regex = Regex::new(r"^\d+\s+-\s+(GOOD|FAIL|SKIP|TIMEOUT)").unwrap();
}

/// Whether a line is Arcadia envelope framing worth keeping.
pub fn keep_framing_line(line: &str, _has_fails: bool) -> bool {
    let t = line.trim_start();
    if t.is_empty() {
        return false;
    }
    // Suite identity headers (must run before the generic `------` chunk rule —
    // `------- [TM]` also starts with `------`).
    if t.contains("[TM]") || t.starts_with("------- [GO]") || t.starts_with("------- [PB]") {
        return true;
    }
    if t.starts_with("------") {
        return t.contains("chunk ran") || t.contains("FAIL") || t.contains("sole chunk");
    }
    if t.starts_with("Total ") {
        return true;
    }
    if t.contains("<py3test>") || t.contains("<go_test>") || t.contains("<gtest>") {
        return true;
    }
    if t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(" - FAIL") {
        return true;
    }
    // Outcome under Total — ya uses tabs; some dumps use spaces. Match content, not indent width.
    if OUTCOME_RE.is_match(t) {
        return true;
    }
    // Final one-word success marker (pass path)
    if t == "Ok" {
        return true;
    }
    false
}

/// Postamble / after fail-block-boundary keep: framing **or** `Log:` / `Logsdir:`.
///
/// Standalone meta lines often appear after a chunk boundary; they must survive
/// even when they are no longer inside a `[fail]` block.
pub fn keep_postamble_line(line: &str, has_fails: bool) -> bool {
    if keep_framing_line(line, has_fails) {
        return true;
    }
    let t = line.trim_start();
    t.starts_with("Log:") || t.starts_with("Logsdir:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_tm_header_despite_dash_prefix() {
        let line = "------- [TM] {default-darwin-arm64, release} path/py3test";
        assert!(keep_framing_line(line, false));
    }

    /// S9-T4: linux platform tags in suite headers are still framing.
    #[test]
    fn keeps_tm_header_linux_platform_variant() {
        let line = "------- [TM] {default-linux-x86_64, release} path/py3test";
        assert!(keep_framing_line(line, false));
        let arm = "------- [TM] {default-linux-arm64, debug} path/gotest";
        assert!(keep_framing_line(arm, false));
    }

    #[test]
    fn keeps_chunk_fail_summary() {
        assert!(keep_framing_line("------ FAIL: 2 - FAIL pay/receiptron/tests", true));
        assert!(keep_framing_line("------ sole chunk ran 2 tests (total:1s)", true));
    }

    #[test]
    fn drops_plain_separator() {
        assert!(!keep_framing_line("------", false));
    }

    #[test]
    fn keeps_tab_indented_good_outcomes() {
        // Real ya dumps use a leading tab under Total lines.
        assert!(keep_framing_line("\t84 - GOOD", false));
        assert!(keep_framing_line("\t255 - GOOD", false));
        assert!(keep_framing_line("        3 - FAIL", true));
        assert!(keep_framing_line("\t1 - SKIP", false));
        assert!(keep_framing_line("Ok", false));
        assert!(!keep_framing_line("GOOD vibes only", false));
    }

    #[test]
    fn postamble_keeps_log_and_logsdir() {
        assert!(keep_postamble_line("Log: /tmp/t.log", true));
        assert!(keep_postamble_line("Logsdir: /tmp/out", true));
        assert!(keep_postamble_line("------ FAIL: 1 - FAIL suite", true));
        assert!(!keep_postamble_line("E   Expected: <spam>", true));
        assert!(!keep_framing_line("Log: /tmp/t.log", true));
    }
}
