#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MIXED: &str = include_str!("fixtures/ya_jsonl/mixed-test.jsonl");
const EXCEPTION: &str = include_str!("fixtures/ya_jsonl/exception-epilogue.jsonl");

fn base_command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rtk"));
    command
        .current_dir(dir)
        .env("RTK_DB_PATH", dir.join("history.db"))
        .env("RTK_TELEMETRY_DISABLED", "1")
        .env("RTK_NO_TOML", "1")
        .env("RTK_TEE_DIR", dir.join("tee"));
    command
}

fn pipe(dir: &Path, filter: &str, input: &str) -> Output {
    let mut child = base_command(dir)
        .args(["pipe", "-f", filter])
        .env("RTK_TEE", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rtk pipe");
    child
        .stdin
        .take()
        .expect("pipe stdin")
        .write_all(input.as_bytes())
        .expect("write pipe input");
    child.wait_with_output().expect("wait for rtk pipe")
}

fn install_fake_ya(dir: &Path, fixture: &str) {
    let fixture_path = dir.join("producer.jsonl");
    fs::write(&fixture_path, fixture).expect("write producer fixture");
    let executable = dir.join("ya");
    fs::write(
        &executable,
        "#!/bin/sh\n\
         printf '%s\\n' 'native stderr preamble' >&2\n\
         cat \"$RTK_JSONL_TEST_FIXTURE\" >&2\n\
         printf '%s\\n' 'native stdout epilogue'\n\
         exit \"$RTK_JSONL_TEST_EXIT\"\n",
    )
    .expect("write fake ya");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("chmod fake ya");
}

fn live(dir: &Path, args: &[&str], fixture: &str, child_status: i32, tee: bool) -> Output {
    install_fake_ya(dir, fixture);
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::iter::once(dir.to_path_buf())
        .chain(std::env::split_paths(&inherited_path))
        .collect::<Vec<PathBuf>>();
    base_command(dir)
        .args(args)
        .env("PATH", std::env::join_paths(path).expect("PATH"))
        .env("RTK_JSONL_TEST_FIXTURE", dir.join("producer.jsonl"))
        .env("RTK_JSONL_TEST_EXIT", child_status.to_string())
        .env("RTK_TEE", if tee { "1" } else { "0" })
        .output()
        .expect("run rtk ya")
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn has_jsonl_summary(shown: &str) -> bool {
    shown.contains("ya test result") || shown.contains("ya make result")
}

fn assert_jsonl_summary(output: &Output, handler: &str, exit_code: i32) {
    let shown = combined(output);
    assert!(shown.contains(&format!("ya {handler} result")), "{shown}");
    assert!(
        shown.contains(&format!("\"exit_code\":{exit_code}")),
        "{shown}"
    );
    assert!(shown.contains("counters"), "{shown}");
    assert!(
        shown.contains("AssertionError: expected 7, got 5"),
        "{shown}"
    );
    assert!(shown.contains("Review the failing assertion"), "{shown}");
    assert!(shown.contains("Use the test filter"), "{shown}");
    assert!(
        !shown.contains("downloaded_bytes"),
        "progress noise leaked: {shown}"
    );
}

#[test]
fn pipe_aliases_render_complete_jsonl_and_keep_pipe_exit_independent() {
    let dir = tempfile::tempdir().expect("tempdir");
    for filter in ["ya", "ya-test", "ya-build"] {
        let result = pipe(dir.path(), filter, MIXED);
        assert!(
            result.status.success(),
            "filter {filter}: {:?}",
            result.status
        );
        assert_jsonl_summary(&result, "test", 10);
        let shown = combined(&result);
        assert!(shown.contains("native warning before start"), "{shown}");
        assert!(shown.contains("native output between events"), "{shown}");
        assert!(shown.contains("test stderr context"), "{shown}");
        assert!(shown.contains("slowest_tests"), "{shown}");
    }
}

#[test]
fn pipe_keeps_finished_status_authoritative_when_fail_records_are_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw = MIXED
        .replace("\"exit_code\":10", "\"exit_code\":0")
        .replace("\"category\":\"test_failed\"", "\"category\":\"success\"");
    let result = pipe(dir.path(), "ya-test", &raw);
    assert!(
        result.status.success(),
        "pipe status is independent: {:?}",
        result.status
    );
    let shown = combined(&result);
    assert!(shown.contains("\"exit_code\":0"), "{shown}");
    assert!(
        shown.contains("AssertionError: expected 7, got 5"),
        "failure remains diagnostic: {shown}"
    );
}

#[test]
fn pipe_rejects_untrusted_jsonl_and_returns_the_original_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    let incomplete = MIXED
        .lines()
        .filter(|line| !line.contains("\"type\":\"finished\""))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let unknown = MIXED.replace("\"type\":\"message\"", "\"type\":\"future\"");
    let malformed = MIXED.replace(
        "native output between events\n",
        "native output between events\n{malformed event}\n",
    );
    let malformed_progress = MIXED.replace("\"active\":1", "\"active\":\"bad\"");

    for raw in [&incomplete, &unknown, &malformed, &malformed_progress] {
        let result = pipe(dir.path(), "ya", raw);
        assert!(
            result.status.success(),
            "pipe itself succeeds: {:?}",
            result.status
        );
        assert_eq!(
            result.stdout,
            raw.as_bytes(),
            "raw fallback must retain input"
        );
        let warning = String::from_utf8_lossy(&result.stderr);
        assert!(
            warning.contains("JSONL"),
            "expected explicit warning, got: {warning}"
        );
    }
}

#[cfg(unix)]
#[test]
fn live_test_and_build_routes_use_jsonl_summary_and_preserve_child_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let test = live(dir.path(), &["ya", "test", "fixture"], MIXED, 10, false);
    assert_eq!(test.status.code(), Some(10));
    assert_jsonl_summary(&test, "test", 10);
    let shown = combined(&test);
    assert!(shown.contains("native stderr preamble"), "{shown}");
    assert!(shown.contains("native stdout epilogue"), "{shown}");

    // A tiny exception plus native prefixes is cheaper verbatim: the usual
    // never-worse guard must retain it, including the child status.
    let tiny = live(dir.path(), &["ya", "make", "fixture"], EXCEPTION, 3, false);
    assert_eq!(tiny.status.code(), Some(3));
    let shown = combined(&tiny);
    assert!(!has_jsonl_summary(&shown), "{shown}");
    assert!(shown.contains(EXCEPTION), "{shown}");

    // The same exception after normal build progress exercises the compact
    // route without requiring RTK to expand the producer's short output.
    let progress =
        "{\"type\":\"progress\",\"stage\":\"build\",\"active\":1,\"done\":0,\"total\":1}\n"
            .repeat(8);
    let build_input = EXCEPTION.replacen('\n', &format!("\n{progress}"), 1);
    let build = live(
        dir.path(),
        &["ya", "make", "fixture"],
        &build_input,
        3,
        false,
    );
    assert_eq!(build.status.code(), Some(3));
    let shown = combined(&build);
    assert!(shown.contains("ya make result"), "{shown}");
    assert!(shown.contains("\"exit_code\":3"), "{shown}");
    assert!(shown.contains("Could not read custom context"), "{shown}");
    assert!(
        shown.contains("Traceback (most recent call last):"),
        "{shown}"
    );
    assert!(shown.contains("ValueError: invalid context"), "{shown}");
    assert!(shown.contains("native stderr preamble"), "{shown}");
    assert!(shown.contains("native stdout epilogue"), "{shown}");
}

#[cfg(unix)]
#[test]
fn live_exit_mismatch_falls_back_to_raw_without_changing_child_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let result = live(dir.path(), &["ya", "test", "fixture"], MIXED, 7, false);
    assert_eq!(result.status.code(), Some(7));
    let shown = combined(&result);
    assert!(
        !has_jsonl_summary(&shown),
        "unverified summary leaked: {shown}"
    );
    assert!(
        shown.contains("\"type\":\"started\""),
        "raw start event lost: {shown}"
    );
    assert!(
        shown.contains("\"type\":\"finished\""),
        "raw final event lost: {shown}"
    );
    let warning = String::from_utf8_lossy(&result.stderr);
    assert!(
        warning.contains("JSONL"),
        "expected explicit warning, got: {warning}"
    );
}

#[cfg(unix)]
#[test]
fn live_unknown_or_incomplete_jsonl_falls_back_without_legacy_compaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let unknown = MIXED.replace("\"type\":\"message\"", "\"type\":\"future\"");
    let incomplete = MIXED
        .lines()
        .filter(|line| !line.contains("\"type\":\"finished\""))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    for raw in [&unknown, &incomplete] {
        let result = live(dir.path(), &["ya", "test", "fixture"], raw, 10, false);
        assert_eq!(result.status.code(), Some(10));
        let shown = combined(&result);
        assert!(
            !has_jsonl_summary(&shown),
            "unverified summary leaked: {shown}"
        );
        assert!(
            shown.contains("\"type\":\"started\""),
            "raw event lost: {shown}"
        );
        assert!(
            shown.contains("\"type\":\"fail\""),
            "failure diagnostic lost: {shown}"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("JSONL"));
    }
}

#[cfg(unix)]
#[test]
fn live_late_json_object_after_emitted_text_does_not_add_a_second_summary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let late = format!(
        "[fail] legacy prefix\nE   AssertionError: old text mode\n------ FAIL: 1 - FAIL suite\n{MIXED}"
    );
    let result = live(dir.path(), &["ya", "test", "fixture"], &late, 10, false);
    assert_eq!(result.status.code(), Some(10));
    let shown = combined(&result);
    assert!(
        !has_jsonl_summary(&shown),
        "late structured stream must not add summary: {shown}"
    );
    assert!(
        shown.contains("legacy prefix"),
        "previously emitted text must remain visible: {shown}"
    );
    assert!(
        shown.contains("\"type\":\"started\""),
        "late JSONL must fall back raw: {shown}"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("follows displayed text"));
}

#[cfg(unix)]
#[test]
fn explicit_machine_and_legacy_output_modes_pass_through_unfiltered() {
    let dir = tempfile::tempdir().expect("tempdir");
    for args in [
        vec!["ya", "make", "-T", "fixture"],
        vec!["ya", "make", "-G", "fixture"],
        vec!["ya", "make", "--output-style=jsonl", "fixture"],
    ] {
        let result = live(dir.path(), &args, MIXED, 10, false);
        assert_eq!(result.status.code(), Some(10), "args={args:?}");
        let shown = combined(&result);
        assert!(
            !has_jsonl_summary(&shown),
            "output mode should stay raw: {shown}"
        );
        assert!(
            shown.contains("\"type\":\"started\""),
            "raw stream missing: {shown}"
        );
        assert!(
            shown.contains("native stderr preamble"),
            "raw preamble missing: {shown}"
        );
    }
}

#[cfg(unix)]
#[test]
fn live_clips_large_diagnostics_only_with_a_complete_saved_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let large = format!(
        "{{\"type\":\"started\",\"handler\":\"test\"}}\n{{\"type\":\"fail\",\"name\":\"large_case\",\"text\":\"{}\"}}\n{{\"type\":\"finished\",\"exit_code\":10,\"category\":\"test_failed\",\"counters\":{{\"tests\":{{\"fail\":1}}}}}}\n",
        "x".repeat(32 * 1024)
    );
    let result = live(dir.path(), &["ya", "test", "fixture"], &large, 10, true);
    assert_eq!(result.status.code(), Some(10));
    let shown = combined(&result);
    assert!(
        shown.len() <= 24 * 1024,
        "not bounded: {} bytes",
        shown.len()
    );
    assert!(
        shown.contains("Raw output:"),
        "recovery path required: {shown}"
    );
    assert!(
        shown.contains("excerpt; see raw line"),
        "physical source location required: {shown}"
    );
    let files: Vec<_> = fs::read_dir(dir.path().join("tee"))
        .expect("tee directory")
        .map(|entry| entry.expect("tee entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect();
    assert_eq!(files.len(), 1);
    let saved = fs::read_to_string(&files[0]).expect("saved raw JSONL");
    assert!(
        saved.contains(&"x".repeat(32 * 1024)),
        "capture must be complete"
    );
}

#[cfg(unix)]
#[test]
fn live_does_not_clip_diagnostics_when_recovery_is_disabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let large = format!(
        "{{\"type\":\"started\",\"handler\":\"test\"}}\n{{\"type\":\"fail\",\"name\":\"large_case\",\"text\":\"{}\"}}\n{{\"type\":\"finished\",\"exit_code\":10,\"counters\":{{}}}}\n",
        "x".repeat(32 * 1024)
    );
    let result = live(dir.path(), &["ya", "test", "fixture"], &large, 10, false);
    assert_eq!(result.status.code(), Some(10));
    let shown = combined(&result);
    assert!(
        !has_jsonl_summary(&shown),
        "must not clip without recovery: {shown}"
    );
    assert!(
        shown.contains(&"x".repeat(32 * 1024)),
        "diagnostic must stay available"
    );
}

#[cfg(unix)]
#[test]
fn live_does_not_clip_diagnostics_when_saved_capture_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("tee"), "block tee directory creation").expect("block tee dir");
    let large = format!(
        "{{\"type\":\"started\",\"handler\":\"test\"}}\n{{\"type\":\"fail\",\"name\":\"large_case\",\"text\":\"{}\"}}\n{{\"type\":\"finished\",\"exit_code\":10,\"counters\":{{}}}}\n",
        "x".repeat(32 * 1024)
    );
    let result = live(dir.path(), &["ya", "test", "fixture"], &large, 10, true);
    assert_eq!(result.status.code(), Some(10));
    let shown = combined(&result);
    assert!(
        !has_jsonl_summary(&shown),
        "must not clip without saved recovery: {shown}"
    );
    assert!(
        shown.contains(&"x".repeat(32 * 1024)),
        "diagnostic must stay available"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("retaining raw output"));
}
