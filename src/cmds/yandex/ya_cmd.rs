//! `ya` CLI proxy — test-mode envelope filter (Stage 2) + passthrough otherwise.
//!
//! `ya make -t*` / `ya test` → `filter_ya_envelope` via `run_filtered`.
//! Build / tool / other → passthrough until later stages.

use crate::cmds::yandex::envelope::{filter_ya_envelope, is_test_mode};
use crate::core::runner;
use crate::core::utils::resolved_command;
use anyhow::Result;
use std::ffi::OsString;

/// First-token subdispatch for filter routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaKind {
    Make,
    Test,
    Tool,
    Other,
}

/// Classify `ya` argv by the first positional token.
pub fn classify(args: &[String]) -> YaKind {
    match args.first().map(String::as_str) {
        Some("make") => YaKind::Make,
        Some("test") => YaKind::Test,
        Some("tool") => YaKind::Tool,
        _ => YaKind::Other,
    }
}

/// Run `ya` — filtered in test mode, passthrough otherwise.
pub fn run(args: &[String], verbose: u8) -> Result<i32> {
    let kind = classify(args);
    let test_mode = is_test_mode(args);

    if verbose > 0 {
        eprintln!(
            "ya {:?} → {}",
            kind,
            if test_mode {
                "test-mode filter"
            } else {
                "passthrough"
            }
        );
    }

    if test_mode {
        let mut cmd = resolved_command("ya");
        for arg in args {
            cmd.arg(arg);
        }
        return runner::run_filtered(
            cmd,
            "ya",
            &args.join(" "),
            filter_ya_safe,
            runner::RunOptions::with_tee("ya"),
        );
    }

    let os_args: Vec<OsString> = args.iter().map(OsString::from).collect();
    runner::run_passthrough("ya", &os_args, verbose)
}

/// Infallible wrapper — on unexpected panic path we still must not block the user.
/// Filter itself is pure and panic-free; this exists for the S2-T9 contract.
fn filter_ya_safe(raw: &str) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| filter_ya_envelope(raw))) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("rtk: ya filter warning: panic in filter_ya_envelope; showing raw output");
            raw.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_make() {
        let args = ["make", "-t", "path"].map(String::from);
        assert_eq!(classify(&args), YaKind::Make);
    }

    #[test]
    fn classify_test() {
        assert_eq!(classify(&["test".into(), "-r".into()]), YaKind::Test);
    }

    #[test]
    fn classify_tool() {
        assert_eq!(classify(&["tool".into(), "dump_json".into()]), YaKind::Tool);
    }

    #[test]
    fn classify_empty_and_other() {
        assert_eq!(classify(&[]), YaKind::Other);
        assert_eq!(classify(&["package".into()]), YaKind::Other);
        assert_eq!(classify(&["-h".into()]), YaKind::Other);
    }
}
