# Yandex / Arcadia ecosystem

> Part of [`src/cmds/`](../README.md) — see also [docs/contributing/TECHNICAL.md](../../../docs/contributing/TECHNICAL.md)

Filters for Arcadia meta-tools: **envelope + inner-runner** for `ya`, compact VCS filters for `arc`.

**Design docs:** [ya-roadmap.md](../../../ya-roadmap.md) (stages 0–10) · [ya-analitycs.md](../../../ya-analitycs.md) (token ROI) · [ACCEPTANCE.md](../../../tests/fixtures/ya/ACCEPTANCE.md) (golden criteria)

| Module | Role |
|--------|------|
| [`ya_cmd.rs`](ya_cmd.rs) | Classify argv → test / build / passthrough; `run_streamed` |
| [`ya_jsonl.rs`](ya_jsonl.rs) / [`ya_output.rs`](ya_output.rs) | Validate agent events, render bounded summaries, preserve raw fallback |
| [`ya_build.rs`](ya_build.rs) | Build-mode progress collapse (`YaBuildStreamFilter`) |
| [`envelope.rs`](envelope.rs) | Test-mode dispatch + live `YaTestStreamFilter` |
| [`framing.rs`](framing.rs) | Suite / chunk / totals keep rules |
| [`detect.rs`](detect.rs) | Fingerprint inner runner from output text |
| [`adapters/`](adapters/) | `py3test`, `go_test`, `generic_fail` |
| [`arc_cmd.rs`](arc_cmd.rs) | `arc status\|log\|diff\|show` (+ other passthrough) |

```
rtk ya make -t … / rtk ya test …
        │
        ▼
┌─ YaTestStreamFilter (live) ─┐
│ fat Expected / Ok / [PB] drop │
│ [fail] → S0-T5 compact live   │
│ no-fail → on_exit envelope    │
└────────────┬─────────────────┘
             ▼
   framing + Log: / Logsdir:
   (overflow → fail_overflow_tee_hint)
             │
             ▼  runner with_tee("ya")
        full raw recovery
```

**Stream vs envelope (important):**

| Path | When | Behavior |
|------|------|----------|
| **Live fail stream** (production) | Suites that emit `[fail]` | **Generic S0-T5 compact only** (`compact_fail_block`) as blocks close — **does not** call py3test/go adapters |
| **No-fail `on_exit`** (production) | Suites with no `[fail]` | [`filter_ya_envelope`](envelope.rs) → may dispatch py3test / go adapters |
| **Buffered oracle** (unit tests) | `filter_ya_envelope` / adapter unit tests | Full adapter pipeline (py3test → pytest reuse; go_test → ya-framed compact) |

In short: **live failure streaming = generic compact**; **language adapters run on the no-fail / oracle path**. Adapter ROI still covers goldens via `filter_ya_envelope` asserts; production fail UX prioritizes low-latency streaming + tee recovery.

## Demo & presentation (token savings)

Contributor design docs above are complete for v1. For **slides / live demos**, use fixtures
(always works) or a real Arcadia tree (`ya` / `arc` on PATH).

### Offline fixture demo (recommended for presentations)

```bash
cargo build
chmod +x scripts/demo-ya-savings.sh
./scripts/demo-ya-savings.sh
```

Or one-liners (`chars/4` ≈ tokens, same metric as analytics):

```bash
# Before/after sizes
RAW=tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt
wc -c "$RAW"
cargo run -q -- pipe -f ya <"$RAW" | wc -c

# Show what the agent would see
cargo run -q -- pipe -f ya <"$RAW" | head -50

# Build spam
cargo run -q -- pipe -f ya-build <tests/fixtures/ya/make_python_py_build_large_proto_raw.txt | head -40

# arc
cargo run -q -- pipe -f arc-status <tests/fixtures/arc/status_raw.txt
cargo run -q -- pipe -f arc-log <tests/fixtures/arc/log_raw.txt
```

Pipe filters: `ya` / `ya-test` · `ya-build` · `arc-status` · `arc-log` · `arc-diff` · `arc-show`.

### Live against real `ya` / `arc`

```bash
cargo install --path .   # or: cargo build --release && export PATH="$PWD/target/release:$PATH"

# Side-by-side (pick a small failing target you already know)
ya make -t path/to/tests 2>&1 | tee /tmp/ya-raw.txt | wc -c
rtk ya make -t path/to/tests 2>&1 | tee /tmp/ya-rtk.txt | wc -c
# Inspect keep invariants
grep -E '\[fail\]|Logsdir:' /tmp/ya-rtk.txt | head

# Build-only
rtk ya make python -r path/to/pkg

# arc
rtk arc status
rtk arc log -n 20
rtk arc diff

# After a few commands — presentation slide for tracked savings
rtk gain
rtk gain -H
rtk gain -g
```

**Hooks (agents auto-rewrite):** `rtk init` / install hooks, then bare `ya make -t …` becomes `rtk ya make -t …`. Confirm with `rtk gain -H`.

**Bypass:** `rtk proxy ya make -t …` — full raw output, still tracked (0% savings).

## Modes

| Invoked as | Pipeline |
|------------|----------|
| `rtk ya test …` | test stream (`YaTestStreamFilter`) |
| `rtk ya make -t` / `-tt` / `-ttX` / `--test …` | test stream |
| `rtk ya make …` (no test flags) | build stream (`YaBuildStreamFilter`) |
| `rtk ya tool …` | passthrough + track |
| other `rtk ya …` | passthrough + track |

### Agent JSONL output

When a filtered `ya make` or `ya test` stream contains object-shaped producer
records, RTK validates it as the `ya` agent JSONL protocol before presenting a
summary. A complete recognized stream shows the producer's final category,
counters and exit code, followed by failure/test diagnostics, useful native
text, and any final guidance. Progress events are omitted from the compact
view. A failure event remains diagnostic evidence; the final `finished` event
is the source of the reported result.

For a live `rtk ya` command, the `finished.exit_code` must match the actual
child-process status. RTK returns that child status unchanged. A malformed,
unknown, incomplete or mismatched stream is shown as captured raw output with a
warning instead of being treated as legacy progress text. If text-mode output
was already emitted before a late JSON object appears, RTK retains the captured
output and does not append a second structured summary.

The same parser is available after the fact through `rtk pipe -f ya`,
`rtk pipe -f ya-test` and `rtk pipe -f ya-build`. Pipe mode validates the
stream's own final event but has no original child process status to compare;
the `rtk pipe` command's exit status remains its own. Invalid or incomplete
JSONL is returned as the original input when it fits the pipe capture limit.

The default structured view is limited to 24 KiB. RTK adds excerpts and
physical raw-line references only when it can save a complete copy of the
captured stream; without complete recovery it keeps raw diagnostics rather
than clipping them away. For live JSONL, a separate bounded event buffer retains
diagnostics and final records throughout the command, independently of the
10 MiB raw capture limit. It discards only validated progress during a recognized
lifecycle. After raw capture fills, RTK still presents every diagnostic and the
verified final result; any saved prefix is explicitly labelled partial and cannot
justify excerpts. If the diagnostic buffer itself fills, RTK streams the retained
records and subsequent non-progress output verbatim, without a second summary.
This fallback can exceed the display budget to preserve diagnostics even when
tee is disabled or unavailable.

Ordinary live source locations
refer to the normalized saved representation (stdout followed by stderr), not
to the original interleaving between descriptors or to byte offsets in the
producer's stream. After raw capture fills, `event line` locations instead refer
to observation order and are not coordinates in the partial tee file.

Live JSONL commands also apply the normal tee policy to compact results. An
eligible failure (or success in `tee.mode = "always"`) can include a saved-output
hint when it fits both the display and never-worse budgets. A size-limited normal
capture is labelled partial; it is never used to justify clipping diagnostics.
Pipe JSONL uses saved recovery when excerpts are needed, without inferring an
original live-process tee policy.

### Detail controls

Ordinary successful agent output contains the final result and counters; repeated
progress is omitted. RTK keeps diagnostics selected by the producer, including
`info` messages: these can contain compiler or test stderr. It does not turn an
error into success because individual failure records were hidden or skipped.

Native `ya` options control which extra details are produced:

```bash
rtk ya make -t path/to/tests --show-slowest-tests 10
rtk ya make -t path/to/tests --show-skipped-tests --show-deselected-tests
rtk ya make -t path/to/tests --inline-diff
```

`--inline-diff` retains full expected/actual failure text. The current `ya` JSONL
reporter does not honor `-P` / `--show-passed-tests`; use a human output style for
passed-test details. Explicit output styles and listing/debug modes pass through
without RTK filtering. To explicitly choose JSONL and compact it, pipe it to the
matching filter (the shell pipeline has its own exit-status rules):

```bash
ya make --output-style=jsonl -t path/to/tests 2>&1 | rtk pipe -f ya-test
ya make --output-style=jsonl path/to/target 2>&1 | rtk pipe -f ya-build
rtk proxy ya make -t path/to/tests  # full producer output
rtk pipe --passthrough < saved-output.jsonl
```

See `rtk help ya` and `rtk pipe --help`. RTK's root `-v` traces routing; it does
not request more test diagnostics.

### Flag passthrough

User filters and verbosity are **never rewritten** — args reach bare `ya` byte-for-byte:

```bash
rtk ya test -F '*order*' -r path/to/tests
rtk ya make -ttX -F '*sku*' path
rtk ya make python -r
```

### Compact policy (S0-T5)

Per `[fail]` keep: headline (node id) · first location · first error head · `Log:` · `Logsdir:` (dedupe OK).

Drop: Expected/but bodies · long stacks · PEERDIR / `[PB]` / `Ok [n/m]` progress when compressing.

### Legacy text truncation recovery

- **Full raw:** runner `RunOptions::with_tee("ya")` only — filters do **not** call `force_tee_hint` on full output (avoids double tee).
- **Capped fail lists** (`CAP_ERRORS`): `fail_overflow_tee_hint` — `force_tee_tail_hint` on the **full** headline list with offset `shown + 1`.

For `rtk ya test` / `rtk ya make -t*`, saved output also includes a short index
of `[fail]` headlines and their **1-based physical lines in that saved file**.
`rtk pipe -f ya` / `ya-test` saves stdin and provides the same index when failures
are present. On Unix, a quoted `sed` command previews 20 lines at the first failure;
use another listed line number to inspect a different failure.

Locations follow the saved file's order: stdin verbatim in pipe mode, stdout
followed by stderr in live mode. They are not offsets in the compact summary
or in the interleaved live display. Only complete lines retained by the tee cap
are indexed; a partial log is identified explicitly. The index uses
`MAX_FAIL_BLOCKS` and reports the first remaining saved failure when capped.
`RTK_TEE=0` disables the saved log and its locations.
The recovery hints respect the output budget: a short live result may show only
the saved-log path; pipe mode retains the usual `never_worse` fallback.

## Adapters

| Fingerprint | Adapter | Notes |
|-------------|---------|--------|
| `py3test` / `test-results/py3test` | `py3test` | **Oracle / no-fail `on_exit` only** (`filter_ya_envelope`): synthetic FAILURES → `filter_pytest_output`. Live fail stream = generic S0-T5 compact. |
| `<go_test>` / `/gotest/` | `go_test` | **Oracle / no-fail `on_exit` only**: ya-framed compact — **not** `go test -json`. Live fail stream = generic S0-T5 compact. |
| vitest / jest | detect only | Fingerprint exists in [`detect.rs`](detect.rs); **no adapter / no fixtures** (follow-up F3). Detected → `generic_fail` / same live compact. |
| unknown (test) | `generic_fail` | Same S0-T5 rules as live fail stream |
| build `ya make` | `ya_build` | Allowlist errors/warnings; collapse progress (`run_streamed`) |

## `arc`

| Invoked as | Pipeline |
|------------|----------|
| `rtk arc status …` | compact like `git status` |
| `rtk arc log …` | oneline bias; default `-n 10` if unset |
| `rtk arc diff …` | normalize → `compact_diff` + tighten |
| `rtk arc show …` | compact header + tightened patch |
| `rtk arc <other> …` | passthrough + track |

```bash
rtk arc status
rtk arc log -n 20
rtk arc diff
rtk arc show HEAD
rtk arc info   # passthrough
```

## Hooks & discover

With RTK hooks installed:

| Raw command | Rewritten to |
|-------------|----------------|
| `ya make …` | `rtk ya make …` |
| `ya test …` | `rtk ya test …` |
| `arc status\|log\|diff\|show …` | `rtk arc …` |
| `ya tool …` | **not rewritten** |
| other `arc …` | **not rewritten** |

**Hook scope (Q4):** always rewrite when hooks are active — same as git/cargo. No PATH or arc-mount gate.

**Known gap — absolute binary paths:** `/usr/local/bin/ya make` classifies as supported but is not rewritten (same gap as `/usr/bin/git status`). Prefer bare `ya` / `arc` on `PATH`, or call `rtk ya` / `rtk arc` explicitly.

Preferred note for Arcadia `AGENTS.md`:

```markdown
## RTK (token-optimized CLI)

Prefer `rtk ya make -t …` / `rtk ya test …` and `rtk arc status|log|diff|show`
over bare `ya` / `arc` when RTK is installed. Do not wrap arbitrary `ya tool *`
unless a dedicated filter exists.
```

Rules: [`src/discover/rules.rs`](../../discover/rules.rs).

## Status

**v1 complete (stages 0–10).** Test + build `ya` streamed; `arc` filtered subset; hooks rewrite safe set; Logsdir / tee / cross-platform / fuzz hardened.

## Non-goals (v1)

- Rewriting RECIPE / parsing all Makefile dialects
- Injecting pytest flags through `ya`
- Blanket `ya tool *` hook rewrite
- JS/vitest under `ya` without fixtures

## Follow-ups (post-v1)

| Item | Notes |
|------|--------|
| Allowlisted `ya tool <name>` | New mini-stage per tool when ROI + fixtures exist; keep wildcard out of hooks |
| Absolute-path rewrite | Platform-wide discover normalization (`/usr/bin/git`, `/usr/local/bin/ya`, …) |
| JS under `ya` | Collect fixtures → `jest_vitest` adapter |
| Dual `ya test` corpus | Only if real dumps diverge from `ya make -t` |

## Related

- Fixtures: [`tests/fixtures/ya/`](../../../tests/fixtures/ya/), [`tests/fixtures/arc/`](../../../tests/fixtures/arc/)
- Acceptance: [`tests/fixtures/ya/ACCEPTANCE.md`](../../../tests/fixtures/ya/ACCEPTANCE.md)
- Roadmap: [`ya-roadmap.md`](../../../ya-roadmap.md)
- Analytics: [`ya-analitycs.md`](../../../ya-analitycs.md)
