//! Shared Arcadia envelope framing helpers (suite/chunk/totals identity lines).

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
    if line.starts_with("        ")
        && (t.contains("FAIL") || t.contains("GOOD") || t.contains("SKIP"))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_tm_header_despite_dash_prefix() {
        let line = "------- [TM] {default-darwin-arm64, release} path/py3test";
        assert!(keep_framing_line(line, false));
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
}
