//! `ya` CLI proxy — test-mode envelope filter + passthrough otherwise.
//!
//! `ya make -t*` / `ya test` share the same filter pipeline (`filter_ya_envelope`).
//! User argv (`-F`, `-r`, `-ttX`, …) is forwarded unchanged — never rewritten.
//! Build / tool / other → passthrough until later stages.

use crate::cmds::yandex::envelope::{filter_ya_envelope, is_test_mode};
use crate::core::runner;
use crate::core::utils::resolved_command;
use anyhow::Result;
use std::ffi::OsString;
use std::process::Command;

/// First-token subdispatch for filter routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaKind {
    Make,
    Test,
    Tool,
    Other,
}

/// Execution plan for `run` — testable without spawning (S4-T1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaPipeline {
    /// `run_filtered` + tee label `"ya"`.
    Filtered { tee: &'static str },
    Passthrough,
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

/// Shared routing: `ya test` and test-mode `ya make` both get filtered+tee.
pub fn pipeline(args: &[String]) -> YaPipeline {
    if is_test_mode(args) {
        YaPipeline::Filtered { tee: "ya" }
    } else {
        YaPipeline::Passthrough
    }
}

/// Child `Command` with user argv forwarded as-is (no rewrite).
fn build_ya_command(args: &[String]) -> Command {
    let mut cmd = resolved_command("ya");
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

/// Run `ya` — filtered in test mode, passthrough otherwise.
pub fn run(args: &[String], verbose: u8) -> Result<i32> {
    let kind = classify(args);

    if verbose > 0 {
        eprintln!(
            "ya {:?} → {}",
            kind,
            match pipeline(args) {
                YaPipeline::Filtered { .. } => "test-mode filter",
                YaPipeline::Passthrough => "passthrough",
            }
        );
    }

    match pipeline(args) {
        YaPipeline::Filtered { tee } => runner::run_filtered(
            build_ya_command(args),
            "ya",
            &args.join(" "),
            filter_ya_safe,
            runner::RunOptions::with_tee(tee),
        ),
        YaPipeline::Passthrough => {
            let os_args: Vec<OsString> = args.iter().map(OsString::from).collect();
            runner::run_passthrough("ya", &os_args, verbose)
        }
    }
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

    /// S4-T1: both routes select `run_filtered` + tee `"ya"` (not just `is_test_mode`).
    #[test]
    fn ya_test_and_make_t_share_filtered_tee_pipeline() {
        let make_t = ["make", "-t", "pay/lib/tests"].map(String::from);
        let ya_test = ["test", "-r", "pay/lib/tests"].map(String::from);
        let filtered = YaPipeline::Filtered { tee: "ya" };
        assert_eq!(pipeline(&make_t), filtered);
        assert_eq!(pipeline(&ya_test), filtered);
        assert_eq!(
            pipeline(&["make", "python", "-r"].map(String::from)),
            YaPipeline::Passthrough
        );
        assert_eq!(
            pipeline(&["tool", "dump"].map(String::from)),
            YaPipeline::Passthrough
        );
    }

    /// S4-T3: flags land on the `Command` that would be spawned (not a to_vec tautology).
    #[test]
    fn build_ya_command_forwards_filter_flags() {
        let args = [
            "test",
            "-F",
            "*order*",
            "-r",
            "pay/lib/tests",
            "-ttX",
        ]
        .map(String::from);
        let cmd = build_ya_command(&args);
        let forwarded: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            forwarded,
            vec![
                "test",
                "-F",
                "*order*",
                "-r",
                "pay/lib/tests",
                "-ttX",
            ]
        );
    }

    #[test]
    fn build_ya_command_forwards_make_ttx_and_filter() {
        let args = ["make", "-ttX", "-F", "*sku*", "pay/lib"].map(String::from);
        let cmd = build_ya_command(&args);
        let forwarded: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(forwarded, args);
        assert_eq!(pipeline(&args), YaPipeline::Filtered { tee: "ya" });
    }
}
