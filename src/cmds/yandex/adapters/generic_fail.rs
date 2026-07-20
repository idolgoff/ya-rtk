//! Generic fail-block compacting (S0-T5 compact policy).
//!
//! Used when no language-specific adapter is selected. Keeps `[fail]` headline,
//! first location line, first error head, `Log:` / `Logsdir:` — drops Expected/but
//! dumps and long stacks.

use crate::core::tee::force_tee_tail_hint;
use crate::core::truncate::CAP_ERRORS;
use lazy_static::lazy_static;
use regex::Regex;

/// Max `[fail]` blocks emitted (remainder → overflow + tee).
pub const MAX_FAIL_BLOCKS: usize = CAP_ERRORS;

/// Tee hint for capped fail headlines.
///
/// `headlines` must be the **full** flat list (shown + hidden). Offset is
/// `shown + 1` so `tail -n +{offset}` lands on the first hidden line — same
/// contract as gh/pnpm/pytest flat-list recovery.
pub fn fail_overflow_tee_hint(headlines: &[&str], shown: usize) -> Option<String> {
    if headlines.len() <= shown {
        return None;
    }
    let content = headlines.join("\n");
    force_tee_tail_hint(&content, "ya-fails", shown + 1)
}

/// Truncate kept lines so a single stack line cannot blow the budget.
pub const MAX_LINE_CHARS: usize = 240;

lazy_static! {
    static ref LOCATION_RE: Regex = Regex::new(r"\.\w+:\d+").unwrap();
}

/// Compact one `[fail]` … block (lines including the `[fail]` headline).
/// Returns `(emitted_lines, truncated_body)`.
pub fn compact_fail_block(block: &[&str]) -> (Vec<String>, bool) {
    if block.is_empty() {
        return (Vec::new(), false);
    }

    let mut out: Vec<String> = Vec::new();
    let mut truncated = false;
    let mut kept_location = false;
    let mut kept_error_head = false;
    let mut kept_log = false;
    let mut kept_logsdir = false;

    for (i, line) in block.iter().enumerate() {
        let trimmed = line.trim_start();

        if i == 0 || trimmed.starts_with("[fail]") {
            out.push(truncate_line(line));
            continue;
        }

        if trimmed.starts_with("Log:") {
            if !kept_log {
                out.push(truncate_line(line));
                kept_log = true;
            }
            continue;
        }

        if trimmed.starts_with("Logsdir:") {
            if !kept_logsdir {
                out.push(truncate_line(line));
                kept_logsdir = true;
            }
            continue;
        }

        // Drop giant pytest diffs
        if trimmed.starts_with("E   Expected:")
            || trimmed.starts_with("E        but:")
            || trimmed.starts_with("E   But:")
        {
            truncated = true;
            continue;
        }

        if !kept_location && is_location_line(trimmed) {
            out.push(truncate_line(line));
            kept_location = true;
            // Location line that already carries the error (go: `file.go:31: msg`)
            if !kept_error_head && looks_like_inline_error(trimmed) {
                kept_error_head = true;
            }
            continue;
        }

        if !kept_error_head && is_error_head(trimmed) {
            out.push(truncate_line(line));
            kept_error_head = true;
            continue;
        }

        // Everything else in the fail body is dropped (stacks, Feature prose, …)
        if !trimmed.is_empty() {
            truncated = true;
        }
    }

    (out, truncated)
}

fn is_location_line(trimmed: &str) -> bool {
    if trimmed.starts_with('E') || trimmed.starts_with('[') {
        return false;
    }
    LOCATION_RE.is_match(trimmed)
}

fn looks_like_inline_error(trimmed: &str) -> bool {
    // `main_test.go:31: non-zero status…`
    LOCATION_RE.is_match(trimmed)
        && (trimmed.contains("failed")
            || trimmed.contains("error")
            || trimmed.contains("Error")
            || trimmed.contains("FAIL")
            || trimmed.contains("non-zero"))
}

fn is_error_head(trimmed: &str) -> bool {
    if trimmed.starts_with("E   Expected:") || trimmed.starts_with("E        but:") {
        return false;
    }
    if trimmed.starts_with("E ") || trimmed.starts_with("E\t") {
        return true;
    }
    if trimmed.starts_with("AssertionError")
        || trimmed.starts_with("Error:")
        || trimmed.starts_with("error:")
    {
        return true;
    }
    // go / cucumber style single-line failure signal
    if trimmed.contains("hook failed:") || trimmed.contains("step error:") {
        return true;
    }
    false
}

/// S0-T5 must-keep signal — never fat-drop these (truncate instead).
fn is_s0_keep_signal(trimmed: &str) -> bool {
    trimmed.starts_with("Log:")
        || trimmed.starts_with("Logsdir:")
        || trimmed.starts_with("[fail]")
        || is_location_line(trimmed)
        || is_error_head(trimmed)
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

/// Slim a line before buffering so multi-MB error heads do not sit in memory.
/// Matches [`compact_fail_block`] truncation (`MAX_LINE_CHARS`).
pub fn slim_line_for_buffer(line: &str) -> String {
    truncate_line(line)
}

/// Split raw text into preamble, fail blocks, and postamble.
pub fn split_fail_sections(raw: &str) -> (Vec<&str>, Vec<Vec<&str>>, Vec<&str>) {
    let lines: Vec<&str> = raw.lines().collect();
    let mut preamble: Vec<&str> = Vec::new();
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut postamble: Vec<&str> = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    let mut seen_fail = false;

    for line in lines {
        if line.trim_start().starts_with("[fail]") {
            seen_fail = true;
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(vec![line]);
            continue;
        }

        if let Some(ref mut block) = current {
            // Structural boundaries end the fail block
            if is_fail_block_boundary(line) {
                blocks.push(std::mem::take(block));
                current = None;
                postamble.push(line);
            } else {
                block.push(line);
            }
        } else if !seen_fail {
            preamble.push(line);
        } else {
            postamble.push(line);
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    (preamble, blocks, postamble)
}

/// Structural boundaries that end a `[fail]` block (chunk/suite framing).
pub fn is_fail_block_boundary(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("------")
        || t.starts_with("Total ")
        || t.starts_with("[TM]")
        || t.starts_with("------- [TM]")
        || t.starts_with("------- [GO]")
        || t.starts_with("------- [PB]")
}

/// Fat assertion / progress lines that dominate multi-MB dumps — safe to drop
/// before buffering (tee + Logsdir recover the rest).
///
/// Pathological length alone does **not** drop S0-T5 keep signals (error head /
/// location / `[fail]` / `Log:` / `Logsdir:`): callers slim those via
/// [`slim_line_for_buffer`] instead of discarding the whole line.
pub fn is_fat_drop_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("E   Expected:")
        || t.starts_with("E        but:")
        || t.starts_with("E   But:")
    {
        return true;
    }
    if t == "Ok" || t.starts_with("Ok [") {
        return true;
    }
    if t.starts_with("------- [PB]") || t.starts_with("-------[PB]") {
        return true;
    }
    if t.contains("PEERDIR") && !t.starts_with("[fail]") {
        return true;
    }
    if t.contains("Unexpected getting uninitialized hot settings") {
        return true;
    }
    // Pathological single-line spam — keep S0-T5 signals (truncate, don't drop)
    if t.len() > 2_000 && !is_s0_keep_signal(t) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_keeps_headline_location_e_logsdir() {
        let block = [
            "[fail] mod::test_x [default-darwin-arm64-debug] (0.1s)",
            "path/to/test.py:10: in test_x",
            "    assert_that(a, equal_to(b))",
            "E   AssertionError: ",
            "E   Expected: <{'huge': 'blob'}>",
            "E        but: was <{'other': 1}>",
            "Log: /tmp/test.log",
            "Logsdir: /tmp/out",
        ];
        let refs: Vec<&str> = block.to_vec();
        let (out, truncated) = compact_fail_block(&refs);
        assert!(truncated);
        let joined = out.join("\n");
        assert!(joined.contains("[fail] mod::test_x"));
        assert!(joined.contains("test.py:10"));
        assert!(joined.contains("E   AssertionError"));
        assert!(!joined.contains("Expected:"));
        assert!(joined.contains("Logsdir:"));
        assert!(joined.contains("Log:"));
    }

    #[test]
    fn fat_drop_line_targets_expected_and_progress() {
        assert!(is_fat_drop_line("E   Expected: <huge>"));
        assert!(is_fat_drop_line("E        but: was <x>"));
        assert!(is_fat_drop_line("Ok [12/100] building"));
        assert!(is_fat_drop_line("------- [PB] proto.spam"));
        assert!(!is_fat_drop_line("Logsdir: /tmp/out"));
        assert!(!is_fat_drop_line("[fail] mod::t [default-linux-x86_64-debug] (0.1s)"));
        assert!(!is_fat_drop_line("Log: /tmp/t.log"));
        // Fat error heads are kept (slimmed), not dropped wholesale (S0-T5)
        let fat_e = format!("E   AssertionError: {}", "x".repeat(2_500));
        assert!(!is_fat_drop_line(&fat_e));
        let slimmed = slim_line_for_buffer(&fat_e);
        assert!(slimmed.chars().count() <= MAX_LINE_CHARS);
        assert!(slimmed.starts_with("E   AssertionError"));
        // Non-signal megabyte spam still drops
        let spam = format!("stack frame junk {}", "y".repeat(2_500));
        assert!(is_fat_drop_line(&spam));
    }

    /// Overflow tee must cover the full headline list; offset = shown + 1.
    #[test]
    fn fail_overflow_tee_uses_full_list_offset() {
        let headlines = [
            "[fail] t0",
            "[fail] t1",
            "[fail] t2",
            "[fail] t3",
            "[fail] t4",
        ];
        // Helper no-ops cleanly when shown covers all
        assert!(fail_overflow_tee_hint(&headlines, headlines.len()).is_none());
        // When tee enabled, hint is tail -n +4 on a 5-line file starting at t0.
        if let Some(hint) = fail_overflow_tee_hint(&headlines, 3) {
            assert!(
                hint.contains("tail -n +4"),
                "offset must be shown+1 on full list, got {hint}"
            );
            assert!(hint.contains("[see remaining:"));
        }
    }

    /// S9-T4: linux platform tag in compact headline is preserved.
    #[test]
    fn compact_preserves_linux_platform_in_headline() {
        let block = [
            "[fail] mod::test_x [default-linux-x86_64-debug] (0.1s)",
            "path.py:10: in test_x",
            "E   AssertionError: boom",
            "Logsdir: /tmp/out",
        ];
        let refs: Vec<&str> = block.to_vec();
        let (out, _) = compact_fail_block(&refs);
        let joined = out.join("\n");
        assert!(joined.contains("default-linux-x86_64-debug"));
        assert!(joined.contains("Logsdir:"));
    }
}
