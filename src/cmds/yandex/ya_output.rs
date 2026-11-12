//! Select structured ya presentation without feeding JSON records to text filters.

use crate::cmds::yandex::ya_jsonl;
use crate::core::{
    guard::never_worse,
    stream::{StreamFilter, RAW_CAP},
    tee,
};

const DISPLAY_LIMIT: usize = 24 * 1024;

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
        let view = never_worse(raw, &view).to_owned();
        // Live commands retain the normal failure/always tee policy. This
        // capture is optional: unlike excerpt recovery, it may be partial,
        // because every diagnostic is already present in the selected view.
        if let Some(exit_code) = expected_exit {
            if let Some(pending) = tee::prepare_tee_saved(raw, "ya", exit_code) {
                let with_hint = format!("{}{}\n", view, pending.saved().hint());
                if with_hint.len() <= DISPLAY_LIMIT
                    && never_worse(raw, &with_hint) == with_hint
                    && pending.persist().is_some()
                {
                    return Some(with_hint);
                }
            }
        }
        return Some(view);
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
    eprintln!(
        "[rtk] ya JSONL exceeds the display budget without complete recovery; retaining raw output"
    );
    Some(raw.to_owned())
}

pub struct YaStreamFilter {
    text: Box<dyn StreamFilter>,
    structured: bool,
    text_emitted: bool,
    capture_truncated: bool,
    progress: ya_jsonl::ProgressState,
    events: Vec<(usize, String)>,
    event_bytes: usize,
    line_number: usize,
    input_bytes: usize,
    emitted_bytes: usize,
    streaming_fallback: bool,
    legacy_prefix_dropped: bool,
}

impl YaStreamFilter {
    pub fn new(text: impl StreamFilter + 'static) -> Self {
        Self {
            text: Box::new(text),
            structured: false,
            text_emitted: false,
            capture_truncated: false,
            progress: ya_jsonl::ProgressState::default(),
            events: Vec::new(),
            event_bytes: 0,
            line_number: 0,
            input_bytes: 0,
            emitted_bytes: 0,
            streaming_fallback: false,
            legacy_prefix_dropped: false,
        }
    }

    fn take_events(&mut self) -> String {
        self.event_bytes = 0;
        self.events
            .drain(..)
            .fold(String::new(), |mut out, (_, line)| {
                out.push_str(&line);
                out.push('\n');
                out
            })
    }

    /// This projection is independent of core's raw prefix. Only validated
    /// progress can disappear; all other records survive even after RAW_CAP.
    fn observe(&mut self, line: &str) -> Option<String> {
        self.line_number += 1;
        self.input_bytes = self
            .input_bytes
            .saturating_add(line.len())
            .saturating_add(1);
        if self.progress.omit(line) {
            return None;
        }
        if self.streaming_fallback {
            return Some(format!("{line}\n"));
        }
        let retained_size = line.len() + std::mem::size_of::<(usize, String)>();
        if !self.legacy_prefix_dropped && self.event_bytes + retained_size <= RAW_CAP {
            self.event_bytes += retained_size;
            self.events.push((self.line_number, line.to_owned()));
            return None;
        }
        if !self.structured {
            // A huge legacy stream must keep using its existing text filter.
            self.events.clear();
            self.event_bytes = 0;
            self.legacy_prefix_dropped = true;
            return None;
        }
        self.streaming_fallback = true;
        eprintln!("[rtk] ya JSONL diagnostic buffer exceeded; streaming non-progress output without excerpts");
        if self.legacy_prefix_dropped {
            eprintln!(
                "[rtk] earlier legacy output exceeded capture limits; retaining subsequent events"
            );
        }
        let mut output = self.take_events();
        output.push_str(line);
        output.push('\n');
        Some(output)
    }

    fn capture_hint(
        &self,
        raw: &str,
        exit_code: i32,
        remaining: usize,
        shown_bytes: usize,
    ) -> Option<String> {
        let mut pending = tee::prepare_tee_saved(raw, "ya", exit_code)?;
        if self.capture_truncated {
            pending.mark_incomplete();
        }
        let hint = format!("{}\n", pending.saved().hint());
        if hint.len() <= remaining
            && shown_bytes.saturating_add(hint.len()).div_ceil(4) <= self.input_bytes.div_ceil(4)
        {
            pending.persist().map(|_| hint)
        } else {
            None
        }
    }

    fn present_after_capture_limit(&mut self, raw: &str, exit_code: i32) -> String {
        eprintln!("[rtk] ya raw capture is incomplete; validated progress omitted, diagnostics retained independently");
        let view = if self.text_emitted {
            None
        } else {
            match ya_jsonl::parse_lines(
                self.events
                    .iter()
                    .map(|(number, line)| (*number, line.as_str())),
                Some(exit_code),
            ) {
                Ok(Some(summary)) => summary.render(DISPLAY_LIMIT, None),
                Ok(None) => None,
                Err(reason) => {
                    eprintln!(
                        "[rtk] ya JSONL not summarized: {reason}; retaining non-progress output"
                    );
                    None
                }
            }
        };
        // A partial capture never permits excerpts. Oversized or rejected
        // projections remain verbatim, including every late diagnostic/final.
        let mut view = view.unwrap_or_else(|| self.take_events());
        let remaining = if view.len() <= DISPLAY_LIMIT {
            DISPLAY_LIMIT - view.len()
        } else {
            DISPLAY_LIMIT
        };
        if let Some(hint) = self.capture_hint(
            raw,
            exit_code,
            remaining,
            self.emitted_bytes.saturating_add(view.len()),
        ) {
            view.push_str(&hint);
        }
        view
    }
}

impl StreamFilter for YaStreamFilter {
    fn feed_line(&mut self, line: &str) -> Option<String> {
        self.structured |= line.trim_start().starts_with('{');
        let observed = self.observe(line);
        if self.structured {
            self.emitted_bytes = self
                .emitted_bytes
                .saturating_add(observed.as_ref().map_or(0, String::len));
            return observed;
        }
        let output = self.text.feed_line(line);
        self.text_emitted |= output.as_ref().is_some_and(|text| !text.is_empty());
        self.emitted_bytes = self
            .emitted_bytes
            .saturating_add(output.as_ref().map_or(0, String::len));
        output
    }

    fn flush(&mut self) -> String {
        if self.structured {
            String::new()
        } else {
            let output = self.text.flush();
            self.text_emitted |= !output.is_empty();
            self.emitted_bytes = self.emitted_bytes.saturating_add(output.len());
            output
        }
    }

    fn handles_tee(&self) -> bool {
        self.structured
    }

    fn on_capture_truncated(&mut self) {
        self.capture_truncated = true;
        self.text.on_capture_truncated();
    }

    fn on_exit(&mut self, exit_code: i32, raw: &str) -> Option<String> {
        if !self.structured {
            return self.text.on_exit(exit_code, raw);
        }
        if self.streaming_fallback {
            return self.capture_hint(raw, exit_code, DISPLAY_LIMIT, self.emitted_bytes);
        }
        if self.capture_truncated {
            return Some(self.present_after_capture_limit(raw, exit_code));
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
