# Yandex / Arcadia ecosystem

> Part of [`src/cmds/`](../README.md) — see also [docs/contributing/TECHNICAL.md](../../../docs/contributing/TECHNICAL.md)

Filters for Arcadia meta-tools. Design: **envelope + inner-runner** (see [`ya-roadmap.md`](../../../ya-roadmap.md)).

| Module | Tool(s) | Stage |
|--------|---------|-------|
| `ya_cmd.rs` | `ya` | 1–6 — classify + test/build pipelines / else passthrough |
| `ya_build.rs` | `ya make` | 6 — build-mode progress collapse + stream handler |
| `envelope.rs` | — | 2–5 — framing + dispatch by `detect` |
| `framing.rs` | — | shared suite/chunk/totals keep rules |
| `detect.rs` | — | 3–5 — fingerprint inner runner from output |
| `adapters/generic_fail.rs` | — | 2 — S0-T5 per-`[fail]` compact |
| `adapters/py3test.rs` | — | 3 — convert ya fails → reuse `filter_pytest_output` |
| `adapters/go_test.rs` | — | 5 — ya-framed go fails + go-other errors |

## Shared pipelines

### Test mode (Stage 4)

`ya make` with `-t` / `-tt` / `-ttX` / `--test` and **`ya test`** → `run_filtered` + envelope adapters.

### Build mode (Stage 6)

`ya make` **without** test flags → [`ya_build`](ya_build.rs) via `run_filtered` (line-oriented [`YaBuildStreamFilter`](ya_build.rs) powers the oracle; `build_stream_filter()` ready for Stage-9 `run_streamed`).

| Invoked as | Pipeline |
|------------|----------|
| `rtk ya test …` | test filter |
| `rtk ya make -t` / `-tt` / `-ttX` / `--test …` | test filter |
| `rtk ya make …` (no test flags) | **build filter** |
| `rtk ya tool …` | passthrough |

### Flag passthrough

User filters and verbosity flags are **never rewritten**:

```bash
rtk ya test -F '*order*' -r path/to/tests
rtk ya make -ttX -F '*sku*' path
rtk ya make python -r
```

`-F`, `-r`, `-ttX`, and any other args reach bare `ya` byte-for-byte.

## Adapters

| Fingerprint | Adapter | Notes |
|-------------|---------|--------|
| `py3test` | `py3test` | Reuses pytest filter on synthetic FAILURES dump |
| `<go_test>` / `/gotest/` | `go_test` | Specialized ya keep — **not** `go test -json` |
| vitest/jest | — | Skipped (no JS fixtures) |
| unknown (test) | `generic_fail` | Headline + location + error head + Log/Logsdir |
| build make | `ya_build` | Progress collapse; stream-friendly |

## Status

**Stage 6:** Build-only `ya make` filtered via `ya_build` + `run_filtered` (stream filter powers the oracle; Stage 9 can switch to `run_streamed`).

**Stage 5:** Go path live. JS optional skipped.

**Stage 4:** `ya test` ≡ test-mode `ya make` for filtering; argv identity preserved.

Compact policy (S0-T5): keep failure node ids + `Logsdir:`; drop Expected/but bodies (via pytest truncation + envelope).

## Non-goals (v1)

- Rewriting RECIPE / parsing all Makefile dialects
- Injecting pytest flags through `ya`
- Blanket `ya tool *` hook rewrite

## Related

- Fixtures: [`tests/fixtures/ya/`](../../../tests/fixtures/ya/)
- Acceptance: [`tests/fixtures/ya/ACCEPTANCE.md`](../../../tests/fixtures/ya/ACCEPTANCE.md)
- Roadmap: [`ya-roadmap.md`](../../../ya-roadmap.md)
