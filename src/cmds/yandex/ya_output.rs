//! Select structured ya presentation without feeding JSON records to text filters.

use crate::cmds::yandex::ya_jsonl;
use crate::core::{guard::never_worse, stream::StreamFilter, tee};

const DISPLAY_LIMIT: usize = 24 * 1024;

pub fn is_legacy(raw: &str) -> bool {
    !raw.lines().any(|line| line.trim_start().starts_with('{'))
}

/// `None` permits the existing text filter. A structured rejection retains raw
/// diagnostics instead of silently treating JSON as a human progress report.
pub fn present_jsonl(raw: &str, expected_exit: Option<i32>) -> Option<String> {
    let summary = match ya_jsonl::parse(raw, expected_exit) {
        Ok(None) => return None,
        Ok(Some(summary)) => summary,
        Err(reason) => {
            eprintln!("[rtk] ya JSONL not summarized: {reason}; retaining raw output");
            return Some(raw.to_owned());
        }
    };
    debug_assert!(expected_exit.is_none_or(|status| status == summary.exit_code()));
    if let Some(view) = summary.render(DISPLAY_LIMIT, None) {
        return Some(never_worse(raw, &view).to_owned());
    }
    if let Some(pending) = tee::prepare_forced_tee(raw, "ya") {
        if pending.saved().is_complete() {
            if let Some(view) = summary.render(DISPLAY_LIMIT, Some(&pending.saved().path)) {
                if never_worse(raw, &view) == view && pending.persist().is_some() {
                    return Some(view);
                }
            }
        }
    }
    eprintln!("[rtk] ya JSONL exceeds the display budget without complete recovery; retaining raw output");
    Some(raw.to_owned())
}

pub struct YaStreamFilter {
    text: Box<dyn StreamFilter>,
    structured: bool,
    text_emitted: bool,
    capture_truncated: bool,
}

impl YaStreamFilter {
    pub fn new(text: impl StreamFilter + 'static) -> Self {
        Self {
            text: Box::new(text),
            structured: false,
            text_emitted: false,
            capture_truncated: false,
        }
    }
}

impl StreamFilter for YaStreamFilter {
    fn feed_line(&mut self, line: &str) -> Option<String> {
        self.structured |= line.trim_start().starts_with('{');
        if self.structured {
            return None;
        }
        let output = self.text.feed_line(line);
        self.text_emitted |= output.as_ref().is_some_and(|text| !text.is_empty());
        output
    }

    fn flush(&mut self) -> String {
        if self.structured {
            String::new()
        } else {
            let output = self.text.flush();
            self.text_emitted |= !output.is_empty();
            output
        }
    }

    fn on_capture_truncated(&mut self) {
        self.capture_truncated = true;
        self.text.on_capture_truncated();
    }

    fn on_exit(&mut self, exit_code: i32, raw: &str) -> Option<String> {
        if !self.structured {
            return self.text.on_exit(exit_code, raw);
        }
        if self.capture_truncated {
            eprintln!("[rtk] ya JSONL capture is incomplete; retaining captured output");
            return Some(raw.to_owned());
        }
        if self.text_emitted {
            // Text filters may already have emitted a prefix. Do not present a
            // second authoritative summary of a stream whose start was displayed.
            eprintln!("[rtk] ya JSONL follows displayed text; retaining captured output");
            return Some(raw.to_owned());
        }
        present_jsonl(raw, Some(exit_code))
    }
}
