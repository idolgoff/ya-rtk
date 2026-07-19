# Yandex / Arcadia ecosystem

> Part of [`src/cmds/`](../README.md) — see also [docs/contributing/TECHNICAL.md](../../../docs/contributing/TECHNICAL.md)

Filters for Arcadia meta-tools. Design: **envelope + inner-runner** (see [`ya-roadmap.md`](../../../ya-roadmap.md)).

| Module | Tool(s) | Stage |
|--------|---------|-------|
| `ya_cmd.rs` | `ya` | 1–4 — classify + shared test-mode filter / else passthrough |
| `envelope.rs` | — | 2–4 — framing + dispatch by `detect` |
| `framing.rs` | — | shared suite/chunk/totals keep rules |
| `detect.rs` | — | 3 — fingerprint inner runner from output |
| `adapters/generic_fail.rs` | — | 2 — S0-T5 per-`[fail]` compact |
| `adapters/py3test.rs` | — | 3 — convert ya fails → reuse `filter_pytest_output` |

## Shared test-mode pipeline (Stage 4)

`ya make` with `-t` / `-tt` / `-ttX` / `--test` and **`ya test`** use the **same** path:

1. `pipeline()` → `Filtered { tee: "ya" }` for both `ya test` and test-mode `ya make`
2. Spawn child `ya` with argv forwarded **unchanged** (`build_ya_command` / `Command::arg`)
3. `run_filtered` + `.tee("ya")` → `filter_ya_envelope` → detect → py3test or generic_fail

| Invoked as | Test mode? |
|------------|------------|
| `rtk ya test …` | always |
| `rtk ya make -t` / `-tt` / `-ttX` / `--test …` | yes |
| `rtk ya make …` (no test flags) | no — passthrough (Stage 6 later) |
| `rtk ya tool …` | no — passthrough |

### Flag passthrough

User filters and verbosity flags are **never rewritten**:

```bash
rtk ya test -F '*order*' -r path/to/tests
rtk ya make -ttX -F '*sku*' path
```

`-F`, `-r`, `-ttX`, and any other args reach bare `ya` byte-for-byte. Filtering is post-hoc on captured output only (no pytest flag injection through `ya`).

## Status

**Stage 4:** `ya test` ≡ test-mode `ya make` for filtering; argv identity preserved.

**Stage 3:** Test-mode output is fingerprinted. `py3test` dumps reuse the pytest filter. Go / unknown still use generic fail until Stage 5+.

Compact policy (S0-T5): keep failure node ids + `Logsdir:`; drop Expected/but bodies (via pytest truncation + envelope).

## Non-goals (v1)

- Rewriting RECIPE / parsing all Makefile dialects
- Injecting pytest flags through `ya`
- Blanket `ya tool *` hook rewrite

## Related

- Fixtures: [`tests/fixtures/ya/`](../../../tests/fixtures/ya/)
- Acceptance: [`tests/fixtures/ya/ACCEPTANCE.md`](../../../tests/fixtures/ya/ACCEPTANCE.md)
- Roadmap: [`ya-roadmap.md`](../../../ya-roadmap.md)
