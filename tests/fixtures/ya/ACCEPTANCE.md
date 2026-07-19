# `ya` golden fixtures — acceptance criteria

CI-critical fixtures for Stages 2+. Remaining files under this directory are **corpus / regression** only.

Decisions: see [ya-roadmap.md § Decisions](../../../ya-roadmap.md#decisions-stage-0).

## Compact policy (S0-T5)

For each `[fail]` (or equivalent failure) block, filtered output **must keep**:

1. The `[fail] …` headline (full node id / test name)
2. First location line (e.g. `path.py:NN: in test_…` or `file.go:NN: …`)
3. First error head (`E …`, `AssertionError:`, or first non-stack error line)
4. `Log:` path when present
5. `Logsdir:` path when present (**never strip**; dedupe spam to once per chunk/suite)

**Must drop / collapse:**

- Giant `Expected:` / `but:` / full pytest diff bodies
- Long stack traces beyond the first error head (tee recovers full raw)
- PEERDIR / compile / `[PB]` proto progress when failures exist
- Passed-test chatter when any failure is present
- Duplicate `Logsdir` lines between chunks

Tee recovery (`RunOptions::….tee("ya")`) is required whenever bodies are truncated.

## Global invariants

- Never drop **all** `[fail]` headlines when the raw input contains any
- Never drop `Logsdir:` when present in raw (may dedupe)
- Unknown inner runner → generic fail filter or raw fallback — never silent loss of failures
- Token metric: `chars/4` or whitespace-token count is fine; use the same metric in all savings asserts
- Min savings below are **floor** targets for the Stage-2+ filter under this compact policy

## Golden set

| ID | Role | Fixture | Mode | Inner | Must keep | May drop | Min savings |
|----|------|---------|------|-------|-----------|----------|-------------|
| G1 | py multi-fail | `make_t_py_fail_logsdir_raw.txt` | test (`-t`) | py3test | All `[fail]` node ids; first location + `E` head per fail; `Log:` / `Logsdir:` | Expected/but dumps; duplicate Logsdir | ≥60% |
| G2 | py large fail (slice) | `make_t_py_fail_large_logsdir_chunk_slice_raw.txt` | test | py3test | Same as G1; suite/chunk identity if present | Chunk progress noise; full assertion bodies | ≥60% |
| G3 | py large chunk | `make_t_py_fail_large_logsdir_chunk_raw.txt` | test | py3test | Same as G1; finals / FAIL counts if present | PEERDIR; passed chatter; body dumps | ≥60% |
| G4 | py pass/mixed | `make_t_py_pass_or_mixed_raw.txt` | test | py3test | Suite identity (`[TM]` / target); warnings that look like real signal if few; totals if present | Deprecation spam volume; redundant headers | ≥40% |
| G5 | go fail | `make_t_go_fail_logsdir_chunk_raw.txt` | test | go_test | `[fail]` headlines; first error/location; `Log:` / `Logsdir:`; chunk/suite FAIL line | Long YDB/dial stacks beyond error head | ≥60% |
| G6 | go other | `make_tt_go_other_raw.txt` | test (`-tt`) | go_test | Suite/chunk identity; any fail or summary signal present | Banner/separator spam | ≥60% |
| G7 | build/proto | `make_python_py_build_large_proto_raw.txt` | build | unk / python build | Real errors/warnings; final command outcome if present | `[PB]` proto spam; `Ok [n/m]` progress; PEERDIR chatter | ≥60% |
| G8 | unk fail | `make_t_unk_fail_raw.txt` | test | unk (compile/tool fail) | Failure signal (exit / `FAILED` / error lines); enough path to locate target | Tool argv noise; repeated compile banners | ≥60% |
| G9 | tool | `tool_unk_other_raw.txt` | tool | unk | Whatever actionable error/status the dump contains (best-effort) | Unrelated agent/transcript chrome if still present | ≥60% |
| G10 | ttX verbose fail | `make_ttX_py_fail_logsdir_chunk_raw.txt` | test (`-ttX`) | py3test | Same as G1 — verbose flags must not defeat fail-block extraction | Extra `-ttX` chatter; full bodies | ≥60% |

## Stage mapping

| Stage | Goldens primarily exercised |
|------:|----------------------------|
| 2 Envelope + generic fail | G1, G5 (then G2–G3, G8, G10) |
| 3 py3test adapter | G1–G4, G10 |
| 5 go adapter | G5, G6 |
| 6 build-only | G7 |
| 4 / tool edge | G9; G10 for `-ttX` |
| 9 streaming / Logsdir audit | G1–G3, G5, G10 (+ synthetic linux / fuzz) |
| 10 docs | — (acceptance unchanged; see roadmap follow-ups) |

## How to assert in tests

```rust
let raw = include_str!("../../../tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt");
let out = filter_ya_envelope(raw); // or full pipeline
assert!(out.contains("[fail]"));
assert!(out.contains("Logsdir:"));
assert!(token_savings(raw, &out) >= 60.0);
```

Prefer `insta` snapshots on G1, G5, and one large (G2 or G3) once Stage 2 is green.
