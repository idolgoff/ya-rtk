//! Presentation of complete ya agent event streams. The producer's final event
//! supplies the status and counters; diagnostic records do not change them.

use serde_json::{Map, Value};
use std::{fmt, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineLabel {
    Raw,
    Event,
}

impl LineLabel {
    fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Event => "event",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Rejection {
    line: Option<usize>,
    reason: &'static str,
    label: LineLabel,
}

impl Rejection {
    fn at(line: usize, reason: &'static str) -> Self {
        Self {
            line: Some(line),
            reason,
            label: LineLabel::Raw,
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{} at {} line {line}", self.reason, self.label.name()),
            None => f.write_str(self.reason),
        }
    }
}

#[derive(Clone, Copy)]
struct Source {
    first: usize,
    last: usize,
    label: LineLabel,
}

impl Source {
    fn line(number: usize, label: LineLabel) -> Self {
        Self {
            first: number,
            last: number,
            label,
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = self.label.name();
        if self.first == self.last {
            write!(f, "{label} line {}", self.first)
        } else {
            write!(f, "{label} lines {}–{}", self.first, self.last)
        }
    }
}

struct Block {
    source: Source,
    text: String,
}

struct FinalEvent {
    source: Source,
    fields: Map<String, Value>,
    exit_code: i32,
}

pub struct Summary {
    result: Block,
    status: String,
    guidance: Option<Block>,
    diagnostics: Vec<Block>,
    exit_code: i32,
    has_failures: bool,
}

#[derive(PartialEq, Eq)]
enum Kind {
    Started,
    Progress,
    Message,
    Fail,
    Tests,
    SlowestTests,
    Finished,
}

impl Kind {
    fn recognize(name: &str) -> Option<Self> {
        Some(match name {
            "started" => Self::Started,
            "progress" => Self::Progress,
            "message" => Self::Message,
            "fail" => Self::Fail,
            "tests" => Self::Tests,
            "slowest_tests" => Self::SlowestTests,
            "finished" => Self::Finished,
            _ => return None,
        })
    }

    fn fields(&self) -> &'static [&'static str] {
        match self {
            Self::Started => &["epoch", "handler"],
            Self::Progress => &["stage", "active", "done", "total", "node", "resources"],
            Self::Message => &["severity", "path", "text", "details"],
            Self::Fail => &["stage", "path", "name", "text", "details", "rerun_filter"],
            Self::Tests => &[
                "path", "suite", "size", "tags", "platform", "tests", "error",
            ],
            Self::SlowestTests => &["tests"],
            Self::Finished => &[
                "exit_code",
                "category",
                "duration",
                "counters",
                "failed",
                "text",
                "tips",
                "advice",
            ],
        }
    }
}

fn object_like(line: &str) -> bool {
    line.trim_start().starts_with('{')
}

fn decode_event(
    line: &str,
    number: usize,
) -> Result<(Kind, String, Map<String, Value>), Rejection> {
    let mut fields: Map<String, Value> =
        serde_json::from_str(line).map_err(|_| Rejection::at(number, "malformed ya JSON event"))?;
    let Some(Value::String(name)) = fields.shift_remove("type") else {
        return Err(Rejection::at(number, "unknown ya event type"));
    };
    let kind =
        Kind::recognize(&name).ok_or_else(|| Rejection::at(number, "unknown ya event type"))?;
    if fields
        .shift_remove("ts")
        .is_some_and(|value| !value.is_number())
    {
        return Err(Rejection::at(number, "invalid ya event timestamp"));
    }
    if fields
        .keys()
        .any(|key| !kind.fields().contains(&key.as_str()))
    {
        return Err(Rejection::at(number, "unknown field in ya event"));
    }
    Ok((kind, name, fields))
}

#[derive(Default)]
enum ProgressPhase {
    #[default]
    BeforeStart,
    Running,
    Closed,
}

/// Track where known progress can be omitted from a live projection.
#[derive(Default)]
pub struct ProgressState {
    phase: ProgressPhase,
}

impl ProgressState {
    /// Return true only for validated progress during a recognized lifecycle.
    /// Callers retain all other lines, including unknown structured records.
    pub fn omit(&mut self, line: &str) -> bool {
        if matches!(self.phase, ProgressPhase::Closed) || !object_like(line) {
            return false;
        }
        let Ok((kind, _, fields)) = decode_event(line, 0) else {
            self.phase = ProgressPhase::Closed;
            return false;
        };
        match (&self.phase, kind) {
            (ProgressPhase::BeforeStart, Kind::Started) if started_handler(&fields, 0).is_ok() => {
                self.phase = ProgressPhase::Running;
            }
            (ProgressPhase::Running, Kind::Progress) if validate_progress(&fields, 0).is_ok() => {
                return true;
            }
            (
                ProgressPhase::Running,
                Kind::Message | Kind::Fail | Kind::Tests | Kind::SlowestTests,
            ) => {}
            _ => self.phase = ProgressPhase::Closed,
        }
        false
    }
}

/// An object-like line selects JSONL validation even if that line is damaged.
/// Pass the original child exit status for live commands when it is available.
pub fn parse(raw: &str, expected_exit: Option<i32>) -> Result<Option<Summary>, Rejection> {
    if !raw.lines().any(object_like) {
        return Ok(None);
    }
    parse_numbered(
        raw.lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line)),
        expected_exit,
        LineLabel::Raw,
    )
}

/// Parse retained live lines at their original 1-based observation positions.
/// These coordinates refer to event arrival order, which can differ from the
/// stdout-then-stderr order of a saved raw capture.
pub fn parse_lines<'a>(
    lines: impl IntoIterator<Item = (usize, &'a str)>,
    expected_exit: Option<i32>,
) -> Result<Option<Summary>, Rejection> {
    parse_numbered(lines, expected_exit, LineLabel::Event).map_err(|mut error| {
        error.label = LineLabel::Event;
        error
    })
}

fn parse_numbered<'a>(
    lines: impl IntoIterator<Item = (usize, &'a str)>,
    expected_exit: Option<i32>,
    label: LineLabel,
) -> Result<Option<Summary>, Rejection> {
    let mut handler = None;
    let mut finished = None;
    let mut failures = Vec::new();
    let mut diagnostics = Vec::new();
    let mut native = None;

    for (number, line) in lines {
        let source = Source::line(number, label);
        if !object_like(line) {
            add_native(&mut native, &mut diagnostics, source, line);
            continue;
        }
        flush_native(&mut native, &mut diagnostics);
        let (kind, name, fields) = decode_event(line, number)?;
        if finished.is_some() {
            return Err(Rejection::at(number, "structured event after ya finished"));
        }
        if kind != Kind::Started && handler.is_none() {
            return Err(Rejection::at(number, "missing ya started event"));
        }
        match kind {
            Kind::Started => {
                if handler.is_some() {
                    return Err(Rejection::at(number, "duplicate ya started event"));
                }
                handler = Some(started_handler(&fields, number)?);
            }
            Kind::Finished => {
                finished = Some(final_event(fields, source, expected_exit)?);
            }
            Kind::Message | Kind::Fail | Kind::Tests | Kind::SlowestTests => {
                let block = diagnostic(fields, kind, &name, source)?;
                if name == "fail" {
                    failures.push(block);
                } else {
                    diagnostics.push(block);
                }
            }
            Kind::Progress => {
                validate_progress(&fields, number)?;
            }
        }
    }
    let Some(handler) = handler else {
        return Ok(None);
    };
    let Some(finished) = finished else {
        return Err(Rejection {
            line: None,
            reason: "incomplete ya event stream",
            label,
        });
    };
    flush_native(&mut native, &mut diagnostics);
    let has_failures = !failures.is_empty();
    failures.extend(diagnostics);
    Ok(Some(Summary::new(
        &handler,
        finished,
        failures,
        has_failures,
    )))
}

fn started_handler(fields: &Map<String, Value>, number: usize) -> Result<String, Rejection> {
    if fields.get("epoch").is_some_and(|value| !value.is_number()) {
        return Err(Rejection::at(number, "invalid ya start epoch"));
    }
    let Some(handler) = fields.get("handler") else {
        return Ok("?".to_owned());
    };
    handler
        .as_str()
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| Rejection::at(number, "invalid ya handler"))
}

/// Discard only recognized progress shapes. Diagnostic payloads are retained
/// separately, but an unexpected nested progress field must not disappear.
fn validate_progress(fields: &Map<String, Value>, number: usize) -> Result<(), Rejection> {
    let valid = fields.iter().all(|(key, value)| match key.as_str() {
        "stage" => value.is_string(),
        "active" | "done" | "total" => value.is_number(),
        "node" => value.as_object().is_some_and(|node| {
            node.iter().all(|(key, value)| match key.as_str() {
                "kind" | "path" => value.is_string() || value.is_null(),
                "elapsed" => value.is_number(),
                _ => false,
            })
        }),
        "resources" => value.as_array().is_some_and(|resources| {
            resources.iter().all(|resource| {
                resource.as_object().is_some_and(|fields| {
                    fields.iter().all(|(key, value)| match key.as_str() {
                        "pattern" | "phase" => value.is_string(),
                        "operation_id" | "downloaded_bytes" | "total_bytes" => value.is_number(),
                        _ => false,
                    })
                })
            })
        }),
        _ => false,
    });
    if valid {
        Ok(())
    } else {
        Err(Rejection::at(number, "invalid ya progress fields"))
    }
}

fn final_event(
    fields: Map<String, Value>,
    source: Source,
    expected_exit: Option<i32>,
) -> Result<FinalEvent, Rejection> {
    let number = source.first;
    let exit_code = fields
        .get("exit_code")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|&value| value >= 0)
        .ok_or_else(|| Rejection::at(number, "invalid ya finished exit_code"))?;
    if expected_exit.is_some_and(|expected| expected != exit_code) {
        return Err(Rejection::at(
            number,
            "ya finished exit_code disagrees with child status",
        ));
    }
    if !fields.get("counters").is_some_and(Value::is_object) {
        return Err(Rejection::at(number, "missing ya final counters"));
    }
    Ok(FinalEvent {
        source,
        fields,
        exit_code,
    })
}

fn diagnostic(
    mut fields: Map<String, Value>,
    kind: Kind,
    name: &str,
    source: Source,
) -> Result<Block, Rejection> {
    let number = source.first;
    let text = match kind {
        Kind::Message | Kind::Fail => {
            let Some(Value::String(text)) = fields.shift_remove("text") else {
                return Err(Rejection::at(number, "missing ya diagnostic text"));
            };
            format!("{name} [{source}] {}\n{text}\n", Value::Object(fields))
        }
        _ => {
            if !fields.get("tests").is_some_and(Value::is_array) {
                return Err(Rejection::at(number, "missing ya test listing"));
            }
            format!("{name} [{source}] {}\n", Value::Object(fields))
        }
    };
    Ok(Block { source, text })
}

fn add_native(
    native: &mut Option<Block>,
    diagnostics: &mut Vec<Block>,
    source: Source,
    line: &str,
) {
    if native
        .as_ref()
        .is_some_and(|block| block.source.last.checked_add(1) != Some(source.first))
    {
        flush_native(native, diagnostics);
    }
    if native.is_none() && line.trim().is_empty() {
        return;
    }
    let block = native.get_or_insert_with(|| Block {
        source,
        text: String::new(),
    });
    block.source.last = source.last;
    block.text.push_str(line);
    block.text.push('\n');
}

fn flush_native(native: &mut Option<Block>, diagnostics: &mut Vec<Block>) {
    if let Some(mut block) = native.take() {
        block
            .text
            .insert_str(0, &format!("native output [{}]\n", block.source));
        diagnostics.push(block);
    }
}

fn nonempty(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        _ => true,
    }
}

impl Summary {
    fn new(
        handler: &str,
        finished: FinalEvent,
        diagnostics: Vec<Block>,
        has_failures: bool,
    ) -> Self {
        let FinalEvent {
            source,
            mut fields,
            exit_code,
        } = finished;
        let mut status = Map::new();
        status.insert("exit_code".to_owned(), Value::from(exit_code));
        if let Some(Value::String(category)) = fields.get("category") {
            if category.len() <= 128 {
                status.insert("category".to_owned(), Value::from(category.as_str()));
            }
        }
        let status = format!("ya result {}\n", Value::Object(status));
        let guidance: Map<_, _> = ["tips", "advice"]
            .into_iter()
            .filter_map(|key| {
                fields
                    .shift_remove(key)
                    .filter(nonempty)
                    .map(|value| (key.to_owned(), value))
            })
            .collect();
        let guidance = (!guidance.is_empty()).then(|| Block {
            source,
            text: format!("finished guidance [{source}] {}\n", Value::Object(guidance)),
        });
        Self {
            result: Block {
                source,
                text: format!("ya {handler} result {}\n", Value::Object(fields)),
            },
            status,
            guidance,
            diagnostics,
            exit_code,
            has_failures,
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    /// Render within the byte budget. A path may name a complete pending capture
    /// while the caller selects a view, but that capture must be persisted before
    /// the view is emitted. This formatter never reads or writes the path.
    /// Without recovery, leave oversized diagnostics to the caller's raw fallback.
    pub fn render(&self, max_bytes: usize, raw_path: Option<&Path>) -> Option<String> {
        let path = raw_path
            .and_then(Path::to_str)
            .filter(|path| !path.is_empty() && !path.chars().any(char::is_control));
        let raw_note = path.map(|path| format!("Raw output: {path}\n"));
        let guidance_note = self.guidance.as_ref().and_then(|block| {
            path.map(|_| format!("Read full ya guidance at {}.\n", block.source))
        });

        if let Some(note) = raw_note.as_deref() {
            if self.has_failures || guidance_note.is_some() {
                if let Some(view) = self.full_view(max_bytes, note, guidance_note.as_deref()) {
                    return Some(view);
                }
            }
        }
        if let Some(view) = self.full_view(max_bytes, "", None) {
            return Some(view);
        }
        self.clipped_view(max_bytes, raw_note.as_deref()?, guidance_note.as_deref())
    }

    fn full_view(
        &self,
        max_bytes: usize,
        raw_note: &str,
        guidance_note: Option<&str>,
    ) -> Option<String> {
        let guidance = guidance_note
            .or(self.guidance.as_ref().map(|block| block.text.as_str()))
            .unwrap_or_default();
        let length = self.result.text.len()
            + self
                .diagnostics
                .iter()
                .map(|block| block.text.len())
                .sum::<usize>()
            + raw_note.len()
            + guidance.len();
        if length > max_bytes {
            return None;
        }
        let mut view = String::with_capacity(length);
        view.push_str(&self.result.text);
        view.push_str(raw_note);
        for block in &self.diagnostics {
            view.push_str(&block.text);
        }
        view.push_str(guidance);
        Some(view)
    }

    fn clipped_view(
        &self,
        max_bytes: usize,
        raw_note: &str,
        guidance_note: Option<&str>,
    ) -> Option<String> {
        // Keep the status readable even when the producer's final payload is large.
        let result_in_header = self.result.text.len() <= max_bytes / 3;
        let mut view = if result_in_header {
            self.result.text.clone()
        } else {
            self.status.clone()
        };
        view.push_str(raw_note);
        view.push_str(guidance_note.unwrap_or_default());
        let budget = max_bytes.checked_sub(view.len())?;
        let blocks: Vec<_> = (!result_in_header)
            .then_some(&self.result)
            .into_iter()
            .chain(&self.diagnostics)
            .collect();
        append_bounded(&mut view, budget, &blocks)?;
        Some(view)
    }
}

fn append_bounded(view: &mut String, budget: usize, blocks: &[&Block]) -> Option<()> {
    const MIN_SHARE: usize = 128;
    if blocks.is_empty() {
        return Some(());
    }
    let mut shown = blocks.len().min(budget / MIN_SHARE);
    let mut note = String::new();
    if shown < blocks.len() {
        // Reserve room before choosing how many source records fit.
        shown = blocks
            .len()
            .min(budget.saturating_sub(MIN_SHARE) / MIN_SHARE);
        let omitted = &blocks[shown..];
        let first = omitted.iter().map(|block| block.source.first).min()?;
        let last = omitted.iter().map(|block| block.source.last).max()?;
        note = format!(
            "[{} entries omitted; see {}]\n",
            omitted.len(),
            Source {
                first,
                last,
                label: omitted.first()?.source.label,
            },
        );
    }
    let share = budget.checked_sub(note.len())? / shown.max(1);
    for block in &blocks[..shown] {
        append_excerpt(view, block, share)?;
    }
    view.push_str(&note);
    Some(())
}

fn append_excerpt(view: &mut String, block: &Block, limit: usize) -> Option<()> {
    let text = &block.text;
    if text.len() <= limit {
        view.push_str(text);
        return Some(());
    }
    let marker = format!("\n[excerpt; see {}]\n", block.source);
    let half = limit.checked_sub(marker.len())? / 2;
    let head = text.floor_char_boundary(half);
    let tail = text.ceil_char_boundary(text.len() - half);
    view.push_str(&text[..head]);
    view.push_str(&marker);
    view.push_str(&text[tail..]);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 24 * 1024;
    const MIXED: &str = include_str!("../../../tests/fixtures/ya_jsonl/mixed-test.jsonl");
    const EXCEPTION: &str =
        include_str!("../../../tests/fixtures/ya_jsonl/exception-epilogue.jsonl");
    const START: &str = r#"{"type":"started","handler":"make"}"#;
    const FINISH: &str = r#"{"type":"finished","exit_code":0,"counters":{}}"#;

    fn read(raw: &str, expected: Option<i32>) -> Summary {
        parse(raw, expected)
            .expect("valid JSONL")
            .expect("recognized JSONL")
    }

    #[test]
    fn discarded_protocol_fields_must_have_recognized_shapes() {
        for record in [
            r#"{"type":"progress","stage":{"error":"bad"},"active":"nonnumeric"}"#,
            r#"{"type":"progress","ts":{"error":"bad"}}"#,
            r#"{"type":"progress","node":{"elapsed":"bad"}}"#,
            r#"{"type":"progress","node":{"error":"bad"}}"#,
            r#"{"type":"progress","resources":[{"phase":{"error":"bad"}}]}"#,
            r#"{"type":"progress","resources":[{"error":"bad"}]}"#,
        ] {
            let raw = format!("{START}\n{record}\n{FINISH}\n");
            assert!(parse(&raw, None).is_err(), "accepted {record}");
        }
        let raw = format!("{{\"type\":\"started\",\"epoch\":{{\"error\":\"bad\"}}}}\n{FINISH}\n");
        assert!(parse(&raw, None).is_err());
    }

    #[test]
    fn numbered_parser_agrees_on_success_and_rejects_incomplete_jsonl() {
        let raw = include_str!("../../../tests/fixtures/ya_jsonl/success.jsonl");
        let numbered = raw.lines().enumerate().map(|(i, line)| (i + 1, line));
        let result = parse_lines(numbered, Some(0))
            .expect("valid JSONL")
            .expect("recognized JSONL");
        assert_eq!(
            result.render(LIMIT, None),
            read(raw, Some(0)).render(LIMIT, None)
        );

        assert!(parse_lines([], None).expect("empty input").is_none());
        assert!(parse_lines([(1, "ordinary text")], None)
            .expect("legacy input")
            .is_none());
        let Err(error) = parse_lines([(7, START)], None) else {
            panic!("accepted an incomplete stream");
        };
        assert_eq!(error.to_string(), "incomplete ya event stream");
    }

    #[test]
    fn streaming_projection_only_omits_valid_progress_between_start_and_finish() {
        let progress = r#"{"type":"progress","ts":1,"stage":"build","done":1,"total":2}"#;
        let mut before = ProgressState::default();
        assert!(!before.omit(progress));

        let mut state = ProgressState::default();
        assert!(!state.omit("native preamble"));
        assert!(!state.omit(START));
        assert!(state.omit(progress));
        assert!(!state.omit(r#"{"type":"message","text":"informational diagnostic"}"#));
        assert!(state.omit(progress));
        assert!(!state.omit(FINISH));
        assert!(!state.omit(progress));

        for invalid in [
            r#"{"type":"progress","ts":"1"}"#,
            r#"{"type":"progress","future":1}"#,
            r#"{"type":"progress","node":{"future":1}}"#,
            r#"{"type":"progress","resources":[{"future":1}]}"#,
            r#"{"type":"progress","#,
        ] {
            let mut state = ProgressState::default();
            assert!(!state.omit(START));
            assert!(!state.omit(invalid), "{invalid}");
            assert!(
                !state.omit(progress),
                "projection is closed after {invalid}"
            );
        }

        let mut invalid_start = ProgressState::default();
        assert!(!invalid_start.omit(r#"{"type":"started","epoch":"1"}"#));
        assert!(!invalid_start.omit(progress));
    }

    #[test]
    fn projection_keeps_event_coordinates_without_rewriting_payloads() {
        let message = r#"{"type":"message","text":"diagnostic references raw line 27"}"#;
        let lines = [
            (2, START),
            (10, "first native block"),
            (12, "second native block"),
            (200, message),
            (300, FINISH),
        ];
        let view = parse_lines(lines, Some(0))
            .expect("valid JSONL")
            .expect("recognized JSONL")
            .render(LIMIT, None)
            .expect("complete view");
        assert!(view.contains("native output [event line 10]\nfirst native block"));
        assert!(view.contains("native output [event line 12]\nsecond native block"));
        assert!(view.contains("message [event line 200]"));
        assert!(view.contains("diagnostic references raw line 27"));

        let Err(error) = parse_lines([(2, START), (300, FINISH)], Some(2)) else {
            panic!("accepted a mismatched child exit");
        };
        assert!(error.to_string().ends_with("at event line 300"));
    }

    #[test]
    fn captured_success_matches_expected_summary_with_fewer_bytes() {
        let raw = include_str!("../../../tests/fixtures/ya_jsonl/success.jsonl");
        let expected = include_str!("../../../tests/fixtures/ya_jsonl/success.summary.txt");
        let view = read(raw, Some(0))
            .render(LIMIT, None)
            .expect("complete view");
        assert_eq!(view, expected);
        assert!(
            view.len() * 5 <= raw.len(),
            "expected at least 80% byte reduction"
        );
    }

    #[test]
    fn final_counters_and_diagnostics_come_from_the_producer() {
        let result = read(MIXED, Some(10));
        assert_eq!(result.exit_code(), 10);
        let view = result.render(LIMIT, None).expect("complete view");
        assert!(view.starts_with("ya test result "));
        assert!(view.contains(r#""tests":{"fail":1,"good":3,"skipped":1}"#));
        assert!(view.contains(r#""failed":{"test":["example/pkg"]}"#));
        assert!(view.contains("fail [raw line 8] "));
        assert!(view.contains("AssertionError: expected 7, got 5"));
        assert!(view.contains(r#""severity":"info""#));
        assert!(view.contains("test stderr context"));
        assert!(view.contains("native output [raw lines 1–2]\nnative warning before start"));
        assert!(view.contains("native output [raw line 6]\nnative output between events"));
        assert!(view.contains("tests [raw line 7]"));
        assert!(view.contains("slowest_tests [raw line 9]"));
        assert!(view.contains("Review the failing assertion"));
        assert!(view.contains("Use the test filter"));
        assert!(!view.contains("downloaded_bytes"));
    }

    #[test]
    fn a_skip_record_does_not_imply_a_failed_run() {
        let skipped = r#"{"type":"fail","text":"test was skipped","details":{"status":"skipped"}}"#;
        let finish = r#"{"type":"finished","exit_code":0,"category":null,"counters":{"tests":{"good":8,"skipped":1}}}"#;
        let raw = format!("{START}\n{skipped}\n{finish}");
        let result = read(&raw, Some(0));
        let view = result.render(LIMIT, None).expect("complete view");
        assert_eq!(result.exit_code(), 0);
        assert!(view.contains(r#""category":null,"counters":{"tests":{"good":8,"skipped":1}}"#));
        assert!(view.contains(r#""status":"skipped""#));
    }

    #[test]
    fn configure_error_category_can_accompany_zero_exit_code() {
        // The configure-only producer path can report the error while exiting zero.
        let failure =
            r#"{"type":"fail","stage":"configure","path":"a","text":"bad macro","details":{}}"#;
        let finish = r#"{"type":"finished","exit_code":0,"category":"configure_error","counters":{"configure_errors":1},"failed":{"configure":["a"]},"advice":"Inspect the configure errors."}"#;
        let raw = format!("{START}\n{failure}\n{finish}");
        let result = read(&raw, Some(0));
        let view = result.render(LIMIT, None).expect("complete view");
        assert_eq!(result.exit_code(), 0);
        assert!(view.contains(r#""exit_code":0,"category":"configure_error""#));
        assert!(view.contains(r#""counters":{"configure_errors":1}"#));
        assert!(view.contains(r#"fail [raw line 2] {"stage":"configure","path":"a","details":{}}"#));
        assert!(view.contains("bad macro"));
        assert!(view.contains("Inspect the configure errors."));
    }

    #[test]
    fn a_null_finished_exit_code_is_invalid_even_without_child_status() {
        let finish = r#"{"type":"finished","exit_code":null,"category":null,"counters":{}}"#;
        let raw = format!("{START}\n{finish}");
        for expected in [None, Some(0)] {
            let Err(error) = parse(&raw, expected) else {
                panic!("accepted an unknown final exit code");
            };
            assert_eq!(
                error.to_string(),
                "invalid ya finished exit_code at raw line 2"
            );
        }
    }

    #[test]
    fn empty_test_listing_and_listing_error_are_retained() {
        let listing = r#"{"type":"tests","path":"a","suite":"pytest","tests":[]}"#;
        let error = r#"{"type":"tests","path":"b","suite":"pytest","tests":[],"error":"Could not list tests"}"#;
        let raw = format!("{START}\n{listing}\n{error}\n{FINISH}");
        let view = read(&raw, Some(0))
            .render(LIMIT, None)
            .expect("complete view");
        assert!(view.contains(r#"tests [raw line 2] {"path":"a","suite":"pytest","tests":[]}"#));
        assert!(view.contains(
            r#"tests [raw line 3] {"path":"b","suite":"pytest","tests":[],"error":"Could not list tests"}"#
        ));
    }

    #[test]
    fn resource_progress_is_omitted_but_info_diagnostics_remain() {
        let start_node = r#"{"type":"progress","stage":"build","active":1,"done":1,"total":3,"node":{"path":"a","kind":"TS","elapsed":0}}"#;
        let download = r#"{"type":"progress","stage":"build","active":1,"done":1,"total":3,"node":{"elapsed":10},"resources":[{"operation_id":1,"pattern":"TOOLCHAIN","phase":"downloading","downloaded_bytes":10},{"operation_id":2,"pattern":"SDK","phase":"downloading","downloaded_bytes":20,"total_bytes":100}]}"#;
        let extract = r#"{"type":"progress","stage":"build","active":1,"done":1,"total":3,"node":{"elapsed":15},"resources":[{"operation_id":1,"pattern":"TOOLCHAIN","phase":"extracting"}]}"#;
        let info = r#"{"type":"message","severity":"info","path":"a","text":"diagnostic text from test stderr","details":{"kind":"TS"}}"#;
        let raw = format!("{START}\n{start_node}\n{download}\n{extract}\n{info}\n{FINISH}");
        let view = read(&raw, Some(0))
            .render(LIMIT, None)
            .expect("complete view");
        assert!(view.contains(
            r#"message [raw line 5] {"severity":"info","path":"a","details":{"kind":"TS"}}"#
        ));
        assert!(view.contains("diagnostic text from test stderr"));
        assert!(!view.contains("progress ["));
        assert!(!view.contains("TOOLCHAIN"));
        assert!(!view.contains("downloaded_bytes"));
        assert!(!view.contains("extracting"));
    }

    #[test]
    fn build_failure_details_and_overflow_counts_are_retained() {
        let failure = r#"{"type":"fail","stage":"build","path":"a","text":"compilation failed","details":{"kind":null,"outputs":["a.o"],"platform":"linux-x86_64","exit_code":2}}"#;
        let finish = r#"{"type":"finished","exit_code":2,"category":"build_failed","counters":{"build_failed":12},"failed":{"build":["a","b","c","d","e","f","g","h","i","j"],"more":{"build":2}}}"#;
        let raw = format!("{START}\n{failure}\n{finish}");
        let view = read(&raw, Some(2))
            .render(LIMIT, None)
            .expect("complete view");
        assert!(view.contains(
            r#""failed":{"build":["a","b","c","d","e","f","g","h","i","j"],"more":{"build":2}}"#
        ));
        assert!(view.contains(
            r#""details":{"kind":null,"outputs":["a.o"],"platform":"linux-x86_64","exit_code":2}"#
        ));
        assert!(view.contains("compilation failed"));
    }

    #[test]
    fn guidance_can_be_read_from_exact_final_line() {
        let result = read(MIXED, None);
        let view = result
            .render(LIMIT, Some(Path::new("/tmp/captured ya.log")))
            .expect("view with source path");
        assert!(view.contains("Raw output: /tmp/captured ya.log"));
        assert!(view.contains("Read full ya guidance at raw line 10"));
        assert!(!view.contains("Review the failing assertion"));
        assert!(view.contains("test stderr context"));
    }

    #[test]
    fn a_native_exception_after_finished_does_not_change_the_status() {
        let result = read(EXCEPTION, Some(3));
        let view = result.render(LIMIT, None).expect("complete view");
        assert!(view.contains(r#""category":"unhandled_exception""#));
        assert!(view.contains("Could not read custom context"));
        assert!(view.contains("native output [raw lines 3–4]"));
        assert!(view.contains("ValueError: invalid context"));
    }

    #[test]
    fn rejects_broken_lifecycle_and_distinguishes_legacy_output() {
        assert!(parse("native text\n[fail] a case", Some(0))
            .expect("legacy allowed")
            .is_none());
        for (raw, reason) in [
            (format!("warning\n{START}\n{{"), "malformed ya JSON event"),
            (
                format!("{START}\n{START}\n{FINISH}"),
                "duplicate ya started",
            ),
            (
                format!("{START}\n{FINISH}\n{FINISH}"),
                "event after ya finished",
            ),
            (format!("{FINISH}\n"), "missing ya started"),
            (format!("{START}\n"), "incomplete ya event stream"),
            (
                format!("{START}\n{{\"type\":\"future\"}}\n{FINISH}"),
                "unknown ya event type",
            ),
            (
                format!("{START}\n{{\"type\":\"progress\",\"future\":1}}\n{FINISH}"),
                "unknown field",
            ),
            (
                format!("{START}\n{{\"type\":\"finished\",\"exit_code\":0}}"),
                "missing ya final counters",
            ),
            (
                format!("{START}\n{{\"type\":\"message\"}}\n{FINISH}"),
                "missing ya diagnostic text",
            ),
        ] {
            let Err(error) = parse(&raw, Some(0)) else {
                panic!("accepted broken stream: {raw}");
            };
            assert!(error.to_string().contains(reason), "{error}");
        }
        let Err(error) = parse(EXCEPTION, Some(0)) else {
            panic!("accepted an inconsistent child exit");
        };
        assert!(error.to_string().contains("disagrees with child status"));
    }

    #[test]
    fn long_diagnostics_are_bounded_only_with_complete_raw_recovery() {
        let raw = format!(
            "{START}\n{{\"type\":\"message\",\"severity\":\"warning\",\"text\":\"first {} last\"}}\n{FINISH}\n",
            "漢".repeat(LIMIT),
        );
        let result = read(&raw, Some(0));
        assert!(result.render(LIMIT, None).is_none());
        let view = result
            .render(LIMIT, Some(Path::new("/tmp/raw.log")))
            .expect("bounded recoverable view");
        assert!(view.len() <= LIMIT);
        assert!(view.contains("first 漢"));
        assert!(view.contains("漢 last"));
        assert!(view.contains("[excerpt; see raw line 2]"));
    }

    #[test]
    fn oversized_final_result_keeps_status_and_points_to_finished_line() {
        let raw = format!(
            "{START}\n{{\"type\":\"finished\",\"exit_code\":3,\"category\":\"unhandled_exception\",\"text\":\"{}\",\"counters\":{{}}}}\n",
            "x".repeat(LIMIT * 2),
        );
        let result = read(&raw, Some(3));
        let view = result
            .render(LIMIT, Some(Path::new("/tmp/raw.log")))
            .expect("bounded recoverable view");
        assert!(view.len() <= LIMIT);
        assert!(
            view.starts_with("ya result {\"exit_code\":3,\"category\":\"unhandled_exception\"}\n")
        );
        assert!(view.contains("[excerpt; see raw line 2]"));
    }

    #[test]
    fn inventory_overflow_names_a_range_covering_omitted_records() {
        let messages = "{\"type\":\"message\",\"text\":\"warning\"}\n".repeat(50);
        let raw = format!("{START}\n{messages}{FINISH}");
        let result = read(&raw, Some(0));
        let view = result
            .render(500, Some(Path::new("/tmp/raw.log")))
            .expect("bounded recoverable inventory");
        assert!(view.len() <= 500);
        assert!(view.contains("entries omitted; see raw lines"));
        assert!(view.contains("–51]"));
        assert!(result.render(500, None).is_none());
    }
}
