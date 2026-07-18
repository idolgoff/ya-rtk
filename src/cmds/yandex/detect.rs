//! Fingerprint the inner test runner from `ya` output text (not from paths alone).

/// Detected nested runner under the Arcadia envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnerRunner {
    Py3test,
    GoTest,
    JestVitest,
    Unknown,
}

/// Scan output for runner fingerprints. Order: py3test → go → js → unknown.
pub fn detect_inner_runner(raw: &str) -> InnerRunner {
    if looks_like_py3test(raw) {
        return InnerRunner::Py3test;
    }
    if looks_like_go_test(raw) {
        return InnerRunner::GoTest;
    }
    if looks_like_jest_vitest(raw) {
        return InnerRunner::JestVitest;
    }
    InnerRunner::Unknown
}

fn looks_like_py3test(raw: &str) -> bool {
    raw.contains("py3test")
        || raw.contains("<py3test>")
        || raw.contains("test-results/py3test")
}

fn looks_like_go_test(raw: &str) -> bool {
    raw.contains("<go_test>")
        || raw.contains("go_test")
        || (raw.contains("=== RUN") && raw.contains(".go:"))
}

fn looks_like_jest_vitest(raw: &str) -> bool {
    (raw.contains("PASS  ") && (raw.contains("vitest") || raw.contains(".test.ts")))
        || (raw.contains("FAIL  ") && raw.contains("vitest"))
        || (raw.contains("Jest") && raw.contains("Tests:"))
        || (raw.contains("Test Files ") && raw.contains("vitest"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_py_fail_fixture() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::Py3test);
    }

    #[test]
    fn detects_py_pass_mixed() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_py_pass_or_mixed_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::Py3test);
    }

    #[test]
    fn detects_go_fail_fixture() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::GoTest);
    }

    #[test]
    fn detects_unk_fail_as_unknown() {
        let raw = include_str!("../../../tests/fixtures/ya/make_t_unk_fail_raw.txt");
        assert_eq!(detect_inner_runner(raw), InnerRunner::Unknown);
    }

    #[test]
    fn empty_is_unknown() {
        assert_eq!(detect_inner_runner(""), InnerRunner::Unknown);
        assert_eq!(detect_inner_runner("hello world\n"), InnerRunner::Unknown);
    }
}
