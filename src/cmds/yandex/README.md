# Yandex / Arcadia ecosystem

> Part of [`src/cmds/`](../README.md) — see also [docs/contributing/TECHNICAL.md](../../../docs/contributing/TECHNICAL.md)

Filters for Arcadia meta-tools. Design: **envelope + inner-runner** (see [`ya-roadmap.md`](../../../ya-roadmap.md)).

| Module | Tool(s) | Stage |
|--------|---------|-------|
| `ya_cmd.rs` | `ya` | 1–3 — classify + test-mode filter / else passthrough |
| `envelope.rs` | — | 2–3 — framing + dispatch by `detect` |
| `framing.rs` | — | shared suite/chunk/totals keep rules |
| `detect.rs` | — | 3 — fingerprint inner runner from output |
| `adapters/generic_fail.rs` | — | 2 — S0-T5 per-`[fail]` compact |
| `adapters/py3test.rs` | — | 3 — convert ya fails → reuse `filter_pytest_output` |

## Status

**Stage 3:** Test-mode output is fingerprinted. `py3test` dumps reuse the pytest filter (no pytest flag injection through `ya`). Go / unknown still use generic fail until later stages.

Compact policy (S0-T5): keep failure node ids + `Logsdir:`; drop Expected/but bodies (via pytest truncation + envelope).

## Non-goals (v1)

- Rewriting RECIPE / parsing all Makefile dialects
- Injecting pytest flags through `ya`
- Blanket `ya tool *` hook rewrite

## Related

- Fixtures: [`tests/fixtures/ya/`](../../../tests/fixtures/ya/)
- Acceptance: [`tests/fixtures/ya/ACCEPTANCE.md`](../../../tests/fixtures/ya/ACCEPTANCE.md)
