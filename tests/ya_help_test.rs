use std::process::Command;

fn help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rtk"))
        .args(args)
        .output()
        .expect("run rtk help");
    assert!(output.status.success(), "{:?}", output.stderr);
    String::from_utf8(output.stdout).expect("UTF-8 help")
}

#[test]
fn ya_help_explains_jsonl_routing_and_native_detail_flags() {
    let text = help(&["help", "ya"]);
    for entry in [
        "summarizes agent JSONL",
        "--show-slowest-tests N",
        "--show-skipped-tests",
        "--show-deselected-tests",
        "--inline-diff",
        "-P with --output-style=ninja",
        "--output-style=jsonl keeps native output",
        "rtk pipe -f ya-build",
        "rtk pipe -f ya-test",
        "rtk proxy ya",
    ] {
        assert!(text.contains(entry), "missing {entry}: {text}");
    }
}

#[test]
fn pipe_help_lists_ya_examples_and_raw_passthrough() {
    let text = help(&["pipe", "--help"]);
    for entry in [
        "ya make --output-style=jsonl TARGET 2>&1 | rtk pipe -f ya-build",
        "ya test --output-style=jsonl TARGET 2>&1 | rtk pipe -f ya-test",
        "rtk pipe --passthrough",
    ] {
        assert!(text.contains(entry), "missing {entry}: {text}");
    }
}
