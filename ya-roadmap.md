# Yandex / Arcadia RTK roadmap

Extend RTK for internal Yandex meta-tools (`ya`, `arc`) using an **envelope + inner-runner** model.

**Context:** [ya-analitycs.md](ya-analitycs.md) (token ROI + design) · fixtures in [`tests/fixtures/ya/`](tests/fixtures/ya/) · filter checklist in [`src/cmds/README.md`](src/cmds/README.md)

**v1 success criteria**

- ≥60% token reduction on failure fixtures
- 100% retention of failed test node ids + `Logsdir`
- Exit codes identical to bare `ya` / `arc`
- Unknown inner runner → generic fail or raw fallback (never silent loss of failures)
- Proxy overhead still &lt;10 ms (excluding child tool)

**Non-goals (v1)**

- Rewriting RECIPE / parsing all Makefile dialects
- Injecting pytest flags through `ya` (prefer post-hoc filtering)
- Rewriting arbitrary `ya tool <anything>` in hooks

---

## Decisions (Stage 0)

| ID | Decision | Rationale |
|----|----------|-----------|
| **S0-T4 / Q1** | Ecosystem dir = [`src/cmds/yandex/`](src/cmds/yandex/) | Matches CLI surface `rtk ya`; parallel to other ecosystem folders |
| **S0-T5 / Q2** | Per-fail keep: **`[fail]` headline + first location line + first `E …` / error head + `Log:` + `Logsdir:`**; drop giant Expected/but / stack bodies (tee recovers full output) | Balances agent signal vs token ROI; assertion dumps dominate fail fixtures |
| **S0-T6 / Q3** | **Assume shared formatter** for `ya test` and `ya make -t` in v1; no separate `ya test` fixtures required now | Corpus is almost all `make_*`; analytics had no measurable `ya test` pastes; Stage 4 validates; add dual fixtures only if divergence appears |

Acceptance criteria for CI-critical fixtures: [`tests/fixtures/ya/ACCEPTANCE.md`](tests/fixtures/ya/ACCEPTANCE.md).

Q4 (hooks always vs arc-mount) stays open — blocked on Stage 8.

---

## Domain model (stable vocabulary)

| Term | Meaning |
|------|---------|
| Envelope | Arcadia framing: suite/chunk headers, `[fail]`, `Logsdir`, totals |
| Inner payload | Nested runner output (pytest, go test, …) |
| Mode | Test (`-t` / `-tt*`) vs build vs tool |
| Adapter | Strip envelope → reuse existing ecosystem filter |
| Generic fail | Fallback: keep `[fail]` blocks + summary only |

```
rtk ya make -t …
        │
        ▼
┌─ Outer envelope ─┐
└────────┬─────────┘
         ▼
┌─ Inner runner (fingerprint) ─┐
│  py3test | go_test | … | unk │
└──────────────────────────────┘
```

---

## Target module layout

```
src/cmds/yandex/          # locked S0-T4
  README.md
  ya_cmd.rs               # clap subdispatch: make | test | tool
  ya_make.rs
  ya_test.rs
  arc_cmd.rs
  envelope.rs
  detect.rs
  adapters/
    py3test.rs
    go_test.rs
    jest_vitest.rs
    generic_fail.rs
```

---

## Stage overview

| Stage | Name | Priority | Status | Depends on |
|------:|------|----------|--------|------------|
| 0 | Fixtures & decisions | P0 | Done | — |
| 1 | Scaffold + CLI shell | P0 | Done | 0 |
| 2 | Envelope + generic fail | P0 | Done | 1 |
| 3 | Inner detect + py3test | P0 | Done | 2 |
| 4 | `ya test` parity + flags | P0 | Not started | 3 |
| 5 | Go (+ optional JS) adapters | P1 | Not started | 3 |
| 6 | Build-only `ya make` | P1 | Not started | 2 |
| 7 | `arc` commands | P1 | Not started | 1 |
| 8 | Hooks & discover | P1 | Not started | 2–4 |
| 9 | Hardening | P2 | Not started | 3–6 |
| 10 | Docs & polish | P2 | Not started | 8 |

Work **one stage at a time**. Within a stage, finish tasks in listed order unless noted parallel.

---

## Stage 0 — Fixtures & decisions

**Goal:** Corpus and open questions resolved so implementation does not thrash.

### Tasks

- [x] **S0-T1** Collect anonymized real `ya` outputs into `tests/fixtures/ya/`
- [x] **S0-T2** Document fixture naming / fingerprints in `tests/fixtures/ya/README.md`
- [x] **S0-T3** Keep collector script (`scripts/collect_ya_fixtures.py`) runnable
- [x] **S0-T4** Ecosystem folder = `src/cmds/yandex/` (see [Decisions](#decisions-stage-0))
- [x] **S0-T5** Compact policy = headline + location + error head + `Log:`/`Logsdir:` (see Decisions)
- [x] **S0-T6** Assume `ya test` ≡ `ya make -t` formatter in v1; dual fixtures only if Stage 4 finds divergence
- [x] **S0-T7** Golden fixture set (G1–G10) locked in [`tests/fixtures/ya/ACCEPTANCE.md`](tests/fixtures/ya/ACCEPTANCE.md)
- [x] **S0-T8** Acceptance table (keep / drop / min savings %) in `ACCEPTANCE.md`

**Exit:** Decisions S0-T4–T6 answered; golden set listed; acceptance bar written. ✅

---

## Stage 1 — Scaffold + CLI shell

**Goal:** `rtk ya …` exists, runs child `ya` unchanged, tracks usage (no filtering yet).

### Tasks

- [x] **S1-T1** Create `src/cmds/yandex/` with `README.md` describing scope
- [x] **S1-T2** Add `ya_cmd.rs` with `pub fn run(args, verbose) -> Result<i32>` via `runner::run_passthrough` (or passthrough-equivalent)
- [x] **S1-T3** Clap: `Commands::Ya { args }` with `trailing_var_arg` + `allow_hyphen_values` in `main.rs`
- [x] **S1-T4** Match arm: route to `ya_cmd::run`
- [x] **S1-T5** Subdispatch sketch: detect first token `make` | `test` | `tool` | other (still passthrough)
- [x] **S1-T6** Smoke: `cargo run -- ya --help` / `rtk ya make -t …` exits with child code
- [x] **S1-T7** Quality gate: `cargo fmt --all && cargo clippy --all-targets && cargo test --all`

**Exit:** `rtk ya <args…>` works as transparent proxy with tracking. ✅

---

## Stage 2 — Envelope + generic fail (P0 win)

**Goal:** Compress Arcadia framing on test-mode output without language detection. Immediate savings on py **and** go fixtures.

### Domain slice

Pure functions first (TDD): `filter_ya_envelope(raw) -> String` and `adapters/generic_fail`.

### Keep

- Suite / target identity
- Chunk one-line status when failures exist
- Each `[fail] …` headline
- Final totals (GOOD/FAIL/TIMEOUT/SKIPPED)
- `Logsdir:` (once per relevant block / not spam)

### Drop / collapse

- PEERDIR / proto / compile progress when failures exist
- Banner / separator spam
- Duplicate `Logsdir` lines
- Passed-test chatter when any failure present

### Tasks

- [x] **S2-T1** RED: unit test on `make_t_py_fail_logsdir_raw.txt` — keeps `[fail]` + `Logsdir`, ≥60% savings
- [x] **S2-T2** RED: same assertions on `make_t_go_fail_logsdir_chunk_raw.txt`
- [x] **S2-T3** RED: empty + malformed input never panic (passthrough or empty `Ok`)
- [x] **S2-T4** GREEN: implement `envelope.rs` state machine / block keeper
- [x] **S2-T5** GREEN: implement `adapters/generic_fail.rs` (used when no inner adapter)
- [x] **S2-T6** Cap oversized assertion / stack bodies per S0-T5; emit tee hint for truncated blocks
- [x] **S2-T7** SNAPSHOT: shape asserts on G1/G5 (project has no insta; locked via unit asserts)
- [x] **S2-T8** Wire test-mode path: if argv has `-t` / `-tt` / `-ttX` / `--test` → `run_filtered` + envelope filter
- [x] **S2-T9** Fallback: filter panic → raw output + stderr warning
- [x] **S2-T10** `.tee("ya")` on filtered path
- [x] **S2-T11** Savings tests for Stage-2 goldens (G1–G3, G5, G8, G10)
- [x] **S2-T12** Quality gate

**Exit:** Test-mode `rtk ya make -t*` filters via envelope; py+go fail fixtures hit ≥60% with Logsdir retained. ✅

---

## Stage 3 — Inner detection + py3test adapter

**Goal:** Fingerprint inner runner; Python path reuses `filter_pytest_output` (largest ROI).

### Fingerprint table (initial)

| Fingerprint | Adapter | Reuse |
|-------------|---------|--------|
| `py3test`, `Logsdir:…/py3test` | `adapters/py3test` | `python::pytest` filter |
| `<go_test>`, go FAIL framing | `adapters/go_test` | (Stage 5) |
| vitest/jest banners | `adapters/jest_vitest` | (Stage 5) |
| unknown | `generic_fail` | Stage 2 |

Detection from **output text**, not directory name alone.

### Tasks

- [x] **S3-T1** RED: `detect.rs` unit tests — py / go / unk fixtures → correct `InnerRunner` enum
- [x] **S3-T2** GREEN: implement fingerprint scanner (order: py3test → go → js → unknown)
- [x] **S3-T3** RED: py3test adapter — after strip, output contains failure signal; ≥60% on large py fixture
- [x] **S3-T4** GREEN: `adapters/py3test.rs` — extract inner payload, call existing pytest filter (or shared core)
- [x] **S3-T5** Pipeline: `envelope` compose with adapter when detected, else `generic_fail`
- [x] **S3-T6** Ensure failed node ids still present after pytest compression
- [x] **S3-T7** SNAPSHOT: large py fail + pass/mixed (shape asserts)
- [x] **S3-T8** Do **not** inject pytest CLI flags through `ya` yet (documented in README)
- [x] **S3-T9** Quality gate

**Exit:** Python `ya make -t` dumps use pytest reuse; detector tested; unknown still safe. ✅

---

## Stage 4 — `ya test` parity + filter args

**Goal:** `rtk ya test …` shares envelope + adapters with `ya make -t`; user filters preserved.

### Tasks

- [ ] **S4-T1** Route `ya test` through same filter pipeline as test-mode `ya make`
- [ ] **S4-T2** Argv passthrough: preserve `-F`, `-r`, `-ttX`, extra flags untouched
- [ ] **S4-T3** Fixture or synthetic argv unit test: `-F '*order*'` still present in spawned command
- [ ] **S4-T4** Confirm `-ttX` still benefits from fail-block extraction (use `make_ttX_py_fail_logsdir_chunk_raw.txt`)
- [ ] **S4-T5** README: document shared pipeline + flag passthrough
- [ ] **S4-T6** Quality gate

**Exit:** `ya test` and `ya make -t` behave equivalently for filtering; filters never rewritten away.

---

## Stage 5 — Go (+ optional JS) adapters

**Goal:** Same detector dispatches Go (and optionally JS) to existing filters.

### Tasks

- [ ] **S5-T1** RED: go fail fixture → adapter keeps fail headlines + Logsdir, ≥60%
- [ ] **S5-T2** GREEN: `adapters/go_test.rs` — strip envelope, reuse `go` test filter where shapes match; else specialized go-under-ya keep
- [ ] **S5-T3** Wire detector → go adapter
- [ ] **S5-T4** SNAPSHOT: go fail + go other
- [ ] **S5-T5** (Optional) Collect JS/TS `ya make -t` fixture if missing; else skip
- [ ] **S5-T6** (Optional) `adapters/jest_vitest.rs` + detector fingerprints
- [ ] **S5-T7** Quality gate

**Exit:** Go path live; JS optional behind same architecture.

---

## Stage 6 — Build-only `ya make`

**Goal:** Non-test `ya make` collapses progress noise; keeps errors/warnings.

### Tasks

- [ ] **S6-T1** Mode detect: absence of `-t`/`-tt*`/`--test` → build mode
- [ ] **S6-T2** RED: `make_python_py_build_large_proto_raw.txt` — ≥60% savings; keep real errors/warnings
- [ ] **S6-T3** GREEN: collapse `Ok [n/m]` / `[PB]` proto spam / PEERDIR chatter
- [ ] **S6-T4** Prefer `run_streamed` + `BlockHandler` if outputs are huge
- [ ] **S6-T5** SNAPSHOT: build golden fixture
- [ ] **S6-T6** Quality gate

**Exit:** Build-mode filter separate from test-mode; large proto dump compressed.

---

## Stage 7 — `arc` commands

**Goal:** Easy wins for high-frequency agent commands (lower token volume, high count).

### Tasks

- [ ] **S7-T1** Collect fixtures: `arc status`, `arc log`, `arc diff`, `arc show` (real dumps → `tests/fixtures/arc/`)
- [ ] **S7-T2** Scaffold `arc_cmd.rs` + `Commands::Arc` in `main.rs`
- [ ] **S7-T3** `arc status` — compact like `git status`
- [ ] **S7-T4** `arc log` — cap / oneline bias like `git log`
- [ ] **S7-T5** `arc diff` / `arc show` — reuse `compact_diff` patterns from git/gh
- [ ] **S7-T6** Other `arc` subcommands: passthrough + track
- [ ] **S7-T7** Savings + snapshot tests per implemented subcommand
- [ ] **S7-T8** Quality gate

**Exit:** `rtk arc status|log|diff|show` filtered; rest tracked passthrough.

---

## Stage 8 — Hooks & discover

**Goal:** Agents auto-rewrite `ya` / selected `arc` to `rtk …`.

### Tasks

- [ ] **S8-T1** Add rewrite patterns in `src/discover/rules.rs` for `ya make`, `ya test`
- [ ] **S8-T2** Add patterns for `arc status|log|diff|show` only
- [ ] **S8-T3** Do **not** rewrite blanket `ya tool *` (passthrough/track only until allowlisted)
- [ ] **S8-T4** Decide hook scope: always rewrite vs only when `ya` on PATH / inside arc mount (document)
- [ ] **S8-T5** Unit/integration tests for rewrite rules
- [ ] **S8-T6** Document preferred `rtk ya make -t …` in ecosystem README (and optional AGENTS.md note for Arcadia projects)
- [ ] **S8-T7** Quality gate

**Exit:** Discover/hooks rewrite safe command set; tool wildcard excluded.

---

## Stage 9 — Hardening

**Goal:** Production robustness for multi-chunk / multi-MB suites.

### Tasks

- [ ] **S9-T1** Streaming path for long test suites (`run_streamed`) — avoid buffering multi-MB logs
- [ ] **S9-T2** Audit: never strip `Logsdir` / per-fail `Log:` paths
- [ ] **S9-T3** Truncation recovery: `force_tee_hint` / `force_tee_tail_hint` for capped fail lists (use `CAP_*`)
- [ ] **S9-T4** Cross-platform string variants in tests (`default-darwin-arm64-debug`, linux if available)
- [ ] **S9-T5** Perf smoke: filtered path overhead acceptable on large fixture (filter-only bench OK)
- [ ] **S9-T6** Fuzz/malformed: binary noise, truncated mid-fail block
- [ ] **S9-T7** Quality gate

**Exit:** Streaming + tee + Logsdir invariants verified.

---

## Stage 10 — Docs & polish

**Goal:** Contributor and user docs match reality.

### Tasks

- [ ] **S10-T1** Finish `src/cmds/yandex/README.md` (modes, adapters, non-goals)
- [ ] **S10-T2** Update top-level command list in `README.md` if required by project convention
- [ ] **S10-T3** Cross-link this roadmap + analytics from ecosystem README
- [ ] **S10-T4** Optional: allowlisted `ya tool <name>` filters case-by-case (new mini-stages per tool)
- [ ] **S10-T5** Mark stages complete in this file; note follow-ups

**Exit:** Docs accurate; roadmap status current.

---

## Engineering backlog (priority order)

Use this when picking the next atomic task:

1. S0-T4…T8 — decisions + golden set
2. S1 — scaffold / passthrough CLI
3. S2 — envelope + generic fail (**first real savings**)
4. S3 — py3test adapter (**largest ROI**)
5. S8-T1 — hook rewrite for `ya make` / `ya test` (can start after S2)
6. S4 — `ya test` parity
7. S7 — `arc` easy wins (parallelizable after S1)
8. S5 — go / JS adapters
9. S6 — build-only `ya make`
10. S9 — streaming + hardening
11. S10 — docs

---

## TDD contract (every filter slice)

```
1. RED      — failing test with real fixture from tests/fixtures/ya/
2. GREEN    — minimum pure filter to pass
3. REFACTOR — lazy_static! regex, shared helpers, no unwrap in prod
4. SAVINGS  — assert ≥60% on that fixture
5. SNAPSHOT — cargo insta review
6. WIRE     — run() + main.rs only after pure filter is green
7. GATE     — cargo fmt --all && cargo clippy --all-targets && cargo test --all
```

Never synthesize fake `ya` output when a fixture exists.

---

## Mapping to existing RTK patterns

| Need | Precedent |
|------|-----------|
| Meta-command → subfilters | `lint_cmd` / `format_cmd` |
| Multi-phase test output | `pytest_cmd`, `CargoTestHandler` |
| Streaming long builds | `run_streamed` + `BlockStreamFilter` |
| Sub-enum CLI | `GitCommands`, `GoCommands`, `DotnetCommands` |
| Hook rewrite | `src/discover/rules.rs` |
| Maven-like mode split | `mvn_cmd` surefire vs compile vs package |

---

## Open questions tracker

| ID | Question | Blocks | Resolution |
|----|----------|--------|------------|
| Q1 | Ecosystem name `yandex/` vs `arcadia/`? | S1 | **`yandex/`** (S0-T4) |
| Q2 | Ultra-compact: body vs headline+Logsdir? | S2-T6 | **Headline + location + error head + Log/Logsdir**; drop Expected/but bodies (S0-T5) |
| Q3 | `ya test` vs `ya make -t` format parity? | S4 | **Assume shared** in v1; re-open if Stage 4 finds divergence (S0-T6) |
| Q4 | Hooks: always vs arc-mount only? | S8-T4 | _TBD_ |

---

## Progress log

| Date | Stage / task | Note |
|------|--------------|------|
| 2026-07-18 | S0-T1…T3 | Fixtures corpus + collector + README done (54 files) |
| 2026-07-18 | — | Roadmap filed as `ya-roadmap.md` |
| 2026-07-18 | S0-T4…T8 | Decisions locked; golden set G1–G10 + `ACCEPTANCE.md`; Stage 0 done |
| 2026-07-18 | S1-T1…T7 | `src/cmds/yandex/` + `Commands::Ya` passthrough; Stage 1 done |
| 2026-07-18 | S2-T1…T12 | Envelope + generic_fail; test-mode `run_filtered` + tee; Stage 2 done |
| 2026-07-18 | S3-T1…T9 | `detect` + py3test→pytest reuse; Stage 3 done |
