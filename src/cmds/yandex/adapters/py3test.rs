//! py3test adapter — strip ya fail framing into synthetic pytest text, then
//! reuse [`filter_pytest_output`](crate::cmds::python::pytest_cmd::filter_pytest_output).
//!
//! Does **not** inject pytest CLI flags through `ya` (Stage 3 non-goal).

use crate::cmds::python::pytest_cmd::filter_pytest_output;
use crate::cmds::yandex::adapters::generic_fail::{
    split_fail_sections, fail_overflow_tee_hint, MAX_FAIL_BLOCKS,
};
use crate::cmds::yandex::framing::keep_postamble_line;

/// Filter py3test-shaped `ya` output. Returns `None` when there are no `[fail]`
/// blocks so the caller can use the generic no-fail envelope path.
pub fn filter_py3test(raw: &str) -> Option<(String, bool)> {
    let has_fail = raw.lines().any(|l| l.trim_start().starts_with("[fail]"));
    if !has_fail {
        return None;
    }

    let (preamble, blocks, postamble) = split_fail_sections(raw);
    let mut out: Vec<String> = Vec::new();

    for line in &preamble {
        if keep_postamble_line(line, true) {
            out.push(line.trim_end().to_string());
        }
    }

    let total = blocks.len();
    let take_n = total.min(MAX_FAIL_BLOCKS);
    let taken: Vec<&Vec<&str>> = blocks.iter().take(take_n).collect();
    let synthetic = ya_blocks_to_pytest(&taken);
    let pytest_out = filter_pytest_output(&synthetic);
    if !pytest_out.is_empty() {
        out.push(pytest_out);
    }

    // Keep Log: (per fail) and Logsdir: (deduped) — pytest path drops them
    let mut seen_logsdir = std::collections::HashSet::new();
    for block in blocks.iter().take(take_n) {
        for line in block.iter() {
            let t = line.trim_start();
            let keep_log = t.starts_with("Log:");
            let keep_logsdir =
                t.starts_with("Logsdir:") && seen_logsdir.insert(t.to_string());
            if keep_log || keep_logsdir {
                out.push(line.trim_end().to_string());
            }
        }
    }

    if total > take_n {
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
        if keep_postamble_line(line, true) {
            out.push(line.trim_end().to_string());
        }
    }

    let mut body = out.join("\n");
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    // Bodies rewritten via pytest filter — always truncated vs raw
    Some((body, true))
}

/// Convert ya `[fail]` blocks into a synthetic pytest FAILURES dump.
fn ya_blocks_to_pytest(blocks: &[&Vec<&str>]) -> String {
    let mut out = String::from("=== FAILURES ===\n");
    let mut node_ids: Vec<String> = Vec::new();

    for block in blocks {
        if block.is_empty() {
            continue;
        }
        let node_id = extract_node_id(block[0]);
        node_ids.push(node_id.clone());
        out.push_str(&format!("___ {} ___\n", node_id));

        for line in block.iter().skip(1) {
            let t = line.trim_start();
            if t.starts_with("Log:") || t.starts_with("Logsdir:") || t.starts_with("[fail]") {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
    }

    out.push_str("=== short test summary info ===\n");
    for id in &node_ids {
        out.push_str(&format!("FAILED {}\n", id));
    }
    out.push_str(&format!(
        "=== 0 passed, {} failed in 0.00s ===\n",
        node_ids.len()
    ));
    out
}

/// `[fail] path::test [platform] (0.1s)` → `path::test`
pub fn extract_node_id(headline: &str) -> String {
    let t = headline.trim_start();
    let t = t.strip_prefix("[fail]").unwrap_or(t).trim();
    if let Some(idx) = t.find(" [") {
        t[..idx].trim().to_string()
    } else {
        t.to_string()
    }
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

    #[test]
    fn extract_node_id_from_headline() {
        let h = "[fail] api.handlers.merchant.test_order.py::test_returned_fiscal_contact [default-darwin-arm64-debug] (0.14s)";
        assert_eq!(
            extract_node_id(h),
            "api.handlers.merchant.test_order.py::test_returned_fiscal_contact"
        );
    }

    /// S9-T4: linux platform tags strip the same way as darwin.
    #[test]
    fn extract_node_id_linux_platform_variant() {
        let h = "[fail] pkg.test_mod.py::test_x [default-linux-x86_64-release] (0.02s)";
        assert_eq!(extract_node_id(h), "pkg.test_mod.py::test_x");
        let arm = "[fail] pkg::TestFoo [default-linux-arm64-debug] (1.0s)";
        assert_eq!(extract_node_id(arm), "pkg::TestFoo");
    }

    #[test]
    fn g1_reuses_pytest_keeps_node_ids_and_logsdir() {
        let raw = include_str!("../../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
        let (out, _) = filter_py3test(raw).expect("G1 has fails");
        assert!(
            out.contains("test_returned_fiscal_contact"),
            "node id must survive pytest compression\n{out}"
        );
        assert!(out.contains("Logsdir:"), "Logsdir must remain\n{out}");
        assert!(out.contains("Log:"), "Log: path must remain\n{out}");
        assert!(
            out.contains("[FAIL]") || out.contains("failed"),
            "failure signal expected\n{out}"
        );
        assert!(
            savings(raw, &out) >= 60.0,
            "savings {:.1}%\n{out}",
            savings(raw, &out)
        );
    }

    #[test]
    fn g3_large_chunk_savings_and_ids() {
        let raw =
            include_str!("../../../../tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_raw.txt");
        let (out, _) = filter_py3test(raw).expect("G3 has fails");
        assert!(out.contains("[FAIL]") || out.contains("failed"));
        assert!(out.contains("Logsdir:"));
        assert!(savings(raw, &out) >= 60.0);
    }

    #[test]
    fn no_fails_returns_none() {
        let raw = include_str!("../../../../tests/fixtures/ya/make_t_py_pass_or_mixed_raw.txt");
        assert!(filter_py3test(raw).is_none());
    }
}
