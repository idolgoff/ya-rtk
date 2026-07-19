# Yandex / Arcadia ecosystem

> Part of [`src/cmds/`](../README.md) — see also [docs/contributing/TECHNICAL.md](../../../docs/contributing/TECHNICAL.md)

Filters for Arcadia meta-tools. Design: **envelope + inner-runner** for `ya` (see [`ya-roadmap.md`](../../../ya-roadmap.md)); compact VCS filters for `arc`.

| Module | Tool(s) | Stage |
|--------|---------|-------|
| `ya_cmd.rs` | `ya` | 1–6, 9 — classify + test/build `run_streamed` / else passthrough |
| `ya_build.rs` | `ya make` | 6+9 — build-mode progress collapse + stream handler |
| `arc_cmd.rs` | `arc` | 7 — status/log/diff/show filtered; other passthrough |
| `envelope.rs` | — | 2–5, 9 — framing + dispatch + `YaTestStreamFilter` |
| `framing.rs` | — | shared suite/chunk/totals keep rules |
| `detect.rs` | — | 3–5 — fingerprint inner runner from output |
| `adapters/generic_fail.rs` | — | 2 — S0-T5 per-`[fail]` compact |
| `adapters/py3test.rs` | — | 3 — convert ya fails → reuse `filter_pytest_output` |
| `adapters/go_test.rs` | — | 5 — ya-framed go fails + go-other errors |

## Shared pipelines

### Test mode (Stage 4 + 9)

`ya make` with `-t` / `-tt` / `-ttX` / `--test` and **`ya test`** → `run_streamed` + [`YaTestStreamFilter`](envelope.rs) (live compact `[fail]` blocks; fat Expected/progress dropped; no-fail suites filter at `on_exit` with `never_worse`).

### Build mode (Stage 6 + 9)

`ya make` **without** test flags → [`ya_build`](ya_build.rs) via `run_streamed` + [`YaBuildStreamFilter`](ya_build.rs).

| Invoked as | Pipeline |
|------------|----------|
| `rtk ya test …` | test stream filter |
| `rtk ya make -t` / `-tt` / `-ttX` / `--test …` | test stream filter |
| `rtk ya make …` (no test flags) | **build stream filter** |
| `rtk ya tool …` | passthrough |

### Flag passthrough

User filters and verbosity flags are **never rewritten**:

```bash
rtk ya test -F '*order*' -r path/to/tests
rtk ya make -ttX -F '*sku*' path
rtk ya make python -r
```

`-F`, `-r`, `-ttX`, and any other args reach bare `ya` byte-for-byte.

## `arc` (Stage 7)

| Invoked as | Pipeline |
|------------|----------|
| `rtk arc status …` | compact like `git status` (strip hints; branch + short entries) |
| `rtk arc log …` | oneline bias; default `-n 10` if unset |
| `rtk arc diff …` | normalize arc unified diff → `compact_diff` + tighten |
| `rtk arc show …` | compact commit header + tightened patch (stat mode when many files) |
| `rtk arc <other> …` | passthrough + track |

```bash
rtk arc status
rtk arc log -n 20
rtk arc diff
rtk arc show HEAD
rtk arc info   # passthrough
```

## Hooks & discover (Stage 8)

With RTK hooks installed, agents' shell commands are rewritten automatically:

| Raw command | Rewritten to |
|-------------|----------------|
| `ya make …` | `rtk ya make …` |
| `ya test …` | `rtk ya test …` |
| `arc status\|log\|diff\|show …` | `rtk arc …` |
| `ya tool …` | **not rewritten** (passthrough until allowlisted) |
| other `arc …` | **not rewritten** |

**Hook scope (Q4):** always rewrite when hooks are active — same as git/cargo. No PATH or arc-mount gate (avoids hook latency; missing binaries fail the same way without RTK).

**Known gap — absolute binary paths:** `classify` strips `/usr/local/bin/ya` → `ya`, but rewrite prefix matching still sees the absolute path, so `/usr/local/bin/ya make` classifies as supported yet is **not** rewritten (same pre-existing gap as `/usr/bin/git status`). Prefer bare `ya` / `arc` on `PATH`, or call `rtk ya` / `rtk arc` explicitly. Fix deferred (platform-wide rewrite normalization).

Preferred explicit form in Arcadia projects (also fine to paste into project `AGENTS.md`):

```markdown
## RTK (token-optimized CLI)

Prefer `rtk ya make -t …` / `rtk ya test …` and `rtk arc status|log|diff|show`
over bare `ya` / `arc` when RTK is installed. Do not wrap arbitrary `ya tool *`
unless a dedicated filter exists.
```

Rules live in [`src/discover/rules.rs`](../../discover/rules.rs).

## Adapters

| Fingerprint | Adapter | Notes |
|-------------|---------|--------|
| `py3test` | `py3test` | Reuses pytest filter on synthetic FAILURES dump |
| `<go_test>` / `/gotest/` | `go_test` | Specialized ya keep — **not** `go test -json` |
| vitest/jest | — | Skipped (no JS fixtures) |
| unknown (test) | `generic_fail` | Headline + location + error head + Log/Logsdir |
| build make | `ya_build` | Progress collapse; stream-friendly |

## Status

**Stage 9:** Test + build modes use `run_streamed`; fat-line slim + `force_tee_*` on capped fails; Logsdir invariants audited.

**Stage 8:** Hooks rewrite `ya make|test` and `arc status|log|diff|show`; `ya tool *` excluded.

**Stage 7:** `rtk arc status|log|diff|show` filtered; other `arc` subcommands passthrough.

**Stage 6:** Build-only `ya make` filtered via `ya_build` + `run_streamed`.

**Stage 5:** Go path live. JS optional skipped.

**Stage 4:** `ya test` ≡ test-mode `ya make` for filtering; argv identity preserved.

Compact policy (S0-T5): keep failure node ids + `Logsdir:`; drop Expected/but bodies.
Truncation recovery: runner tee `"ya"` for full raw; capped fail lists use
`fail_overflow_tee_hint` (`force_tee_tail_hint` on the **full** headline list with
offset `CAP_ERRORS + 1`). Filters do **not** embed `force_tee_hint` for full raw
(avoids double/triple tee with the runner).

## Non-goals (v1)

- Rewriting RECIPE / parsing all Makefile dialects
- Injecting pytest flags through `ya`
- Blanket `ya tool *` hook rewrite

## Related

- Fixtures: [`tests/fixtures/ya/`](../../../tests/fixtures/ya/), [`tests/fixtures/arc/`](../../../tests/fixtures/arc/)
- Acceptance: [`tests/fixtures/ya/ACCEPTANCE.md`](../../../tests/fixtures/ya/ACCEPTANCE.md)
- Roadmap: [`ya-roadmap.md`](../../../ya-roadmap.md)
- Analytics: [`ya-analitycs.md`](../../../ya-analitycs.md)
