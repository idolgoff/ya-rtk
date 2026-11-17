//! Read-only ya argv classification; native parser still owns validation.

#[derive(Default)]
pub(super) struct RoutingOptions {
    pub(super) tests: bool,
    pub(super) native_output: bool,
}

fn explicit_native_output(arg: &str) -> bool {
    matches!(
        arg,
        "--version"
            | "--dry-run"
            | "--list-tests"
            | "--pdb"
            | "--gdb"
            | "--dlv"
            | "--test-debug"
            | "--test-stdout"
            | "--test-stderr"
            | "--teamcity"
    ) || [
        "--help",
        "--json",
        "--dump",
        "--print",
        "--stdout",
        "--output-format",
        "--output-style",
        "--format",
        "--canon-diff",
        "--html-display",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
}

/// Inspect the output-affecting options without rewriting ya's argv.
pub(super) fn routing_options(args: &[String]) -> RoutingOptions {
    let mut options = RoutingOptions::default();
    match args.first().map(String::as_str) {
        Some("test") => options.tests = true,
        Some("make") => {}
        _ => return options,
    }

    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--test-filter" {
            let _ = args.next();
        } else if matches!(arg.as_str(), "--test" | "--run-tests" | "--run-all-tests") {
            options.tests = true;
        } else if explicit_native_output(arg) {
            options.native_output = true;
        } else if let Some(flags) = arg.strip_prefix('-').filter(|s| !s.starts_with('-')) {
            let bytes = flags.as_bytes();
            for (index, flag) in bytes.iter().enumerate() {
                match flag {
                    b't' | b'A' => options.tests = true,
                    b'L' | b'T' | b'G' | b'h' => options.native_output = true,
                    b'P' | b'X' => {}
                    b'F' | b'j' => {
                        // The rest of this argument is an attached value, or the
                        // next argument supplies it even when it resembles a flag.
                        if index + 1 == bytes.len() {
                            let _ = args.next();
                        }
                        break;
                    }
                    _ => {
                        // An unknown letter could consume a value or start a
                        // cluster. Let ya handle ambiguous multi-letter arguments.
                        options.native_output |= bytes.len() > 1;
                        break;
                    }
                }
            }
        }
    }
    options
}

/// True when known argv options request tests.
pub fn is_test_mode(args: &[String]) -> bool {
    routing_options(args).tests
}
