//! Generic fail-block compacting (S0-T5 compact policy).
//!
//! Used when no language-specific adapter is selected. Keeps `[fail]` headline,
//! first location line, first error head, `Log:` / `Logsdir:` — drops Expected/but
//! dumps and long stacks.

use crate::core::truncate::CAP_ERRORS;
use lazy_static::lazy_static;
use regex::Regex;

/// Max `[fail]` blocks emitted (remainder → overflow + tee).
pub const MAX_FAIL_BLOCKS: usize = CAP_ERRORS;

/// Truncate kept lines so a single stack line cannot blow the budget.
const MAX_LINE_CHARS: usize = 240;

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

fn truncate_line(line: &str) -> String {
    let t = line.trim_end();
    if t.chars().count() <= MAX_LINE_CHARS {
        return t.to_string();
    }
    let mut s: String = t.chars().take(MAX_LINE_CHARS.saturating_sub(1)).collect();
    s.push('…');
    s
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

fn is_fail_block_boundary(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("------")
        || t.starts_with("Total ")
        || t.starts_with("[TM]")
        || t.starts_with("------- [TM]")
        || t.starts_with("------- [GO]")
        || t.starts_with("------- [PB]")
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
}
