# Yandex tools × RTK — analytics & roadmap

Date: 2026-07-18  
Source workspace transcripts:  
`~/.cursor/projects/Users-idolgoff-workspace-yandex-pay-plus-yandex-pay-plus-code-workspace/agent-transcripts/`

Token estimate used throughout: **chars / 4** (LLM-ish heuristic).  
Deduped identical pastes.

---

## Caveats (read first)

1. **Shell tool results are not persisted** in those Cursor agent transcripts — only `tool_use` (the command) is logged. Measurable “incoming tokens” come from **user terminal/code pastes** into the agent.
2. Exact flags are often missing from pastes; Arcadia test dumps (`py3test`, `Logsdir:`, `[fail] … darwin`) are treated as the **`ya make -t*`** family.
3. Agents also ran `ya test -r` / `ya test -r -F` via Shell (outputs not measurable here). Those likely add more volume of the same class.

---

## Part 1 — Analytics: largest Yandex-tool LLM input

### Ranked by estimated incoming tokens (DESC)

| # | Tool / pattern | Σ est. tokens | Pastes | Notes |
|---|----------------|--------------:|-------:|-------|
| 1 | **`ya make -t*`** (test output; flags often omitted) | **~66 000** | 27 | Dominant. `py3test` / `[fail]` / `Logsdir:` dumps. Largest single paste ~**22 300** tok |
| 2 | **`ya make -tt -F`** | **~11 000** | 1 | Explicit: `ya make -tt -F '*order*'` |
| 3 | **`ya` test-results log file** (pasted/attached) | **~8 300** | 2 | Files under `…/test-results/py3test/…` |
| 4 | **`ya make -ttX`** | **~7 500** | 1 | Explicit in user message |
| 5 | Other ya-related / unclassified | **~4 000** | 2 | pytest-style failure blobs without clear cmd |

**Combined `ya make -t` / `-tt` / `-ttX` family ≈ ~85 000 tokens** — essentially all heavy LLM input in these logs.

### Top individual pastes

| # | Est. tokens | Attribution |
|--:|------------:|-------------|
| 1 | 22 312 | `ya make -t*` (inferred) — multi-fail `py3test` dump |
| 2 | 10 973 | `ya make -tt -F '*order*'` |
| 3 | 8 116 | test-results log file paste |
| 4 | 7 522 | `ya make -t*` (inferred) |
| 5 | 7 503 | `ya make -ttX` |
| 6 | 7 167 | `ya make -t*` (inferred) |
| 7 | 5 073 | `ya make -t*` (inferred) — suite summary + fails |
| 8 | 3 784 | `ya make -t*` (inferred) |

### Agent Shell invocations (frequency only; output size unknown)

| Count | Command family |
|------:|----------------|
| 25 | `ya test -r -F` |
| 9 | `ya test -r` |
| 8 | `arc show` |
| 6 | `arc log` |
| 5 | `arc diff` |
| ≤3 | `arc root` / `arc status` / other |

### Implication for RTK

**P0 savings target is not “ya” in general — it is the Arcadia test envelope + inner test runner noise**, especially Python `py3test` failure traces and chunk summaries. Build-progress / PEERDIR noise is secondary in this sample but still worth filtering for `ya make` without `-t`.

---

## Part 2 — Roadmap: extend RTK for Yandex (`ya` / `arc`) tools

### Design acknowledgement: `ya make -t` is a meta-runner

`ya make -t` (and `-tt`, `-ttX`, `-F`, filters) is **not one tool**. It is an **orchestrator**:

```
rtk ya make -t <target>
        │
        ▼
┌───────────────────────────────┐
│ Outer envelope (always ya)    │  PEERDIR / compile / link / suite headers /
│                               │  chunk timing / Logsdir / Total N suite /
│                               │  FAIL|GOOD|TIMEOUT counts
└───────────────┬───────────────┘
                │  recipe from ya.make / Makefiles / project macros
                ▼
┌───────────────────────────────┐
│ Inner runner (project-type)   │
│  • Python  → py3test / pytest │  ← dominates token volume in analytics
│  • Go      → go test          │
│  • JS/TS   → jest / vitest /  │
│              hermione / …     │
│  • C++/…   → gtest / custom   │
│  • Custom  → anything Makefile│
│              RECIPE invokes   │
└───────────────────────────────┘
```

**Consequences for RTK:**

1. **Do not treat `ya make -t` as a single monolithic filter.** Split **outer envelope** vs **inner payload**.
2. **Do not assume language from the path alone.** Prefer **output fingerprinting** (and optional target metadata) over guessing from directory name — Makefiles can wrap or replace the default runner.
3. **Reuse existing ecosystem filters** (`pytest_cmd`, `cargo` test, `go test`, `vitest`, …) on the inner payload when fingerprints match — do not reimplement pytest compression inside a `ya` module.
4. **Flag injection is harder than for bare `pytest`.** RTK today injects `--tb=short -q` into pytest. Under `ya make -t`, pytest flags often go through **`-F` / `--test-param` / RECIPE env**, not argv. Prefer **post-hoc filtering** first; flag injection only where Arcadia docs/local conventions allow and tests prove it does not break the recipe.
5. **Fail-safe:** if outer/inner parsing fails → raw output (existing RTK contract).

---

### Proposed module layout

New ecosystem under `src/cmds/` (3+ related commands justify it — see `src/cmds/README.md`):

```
src/cmds/yandex/          # or arcadia/
  README.md
  mod.rs
  ya_cmd.rs               # clap subdispatch: make | test | tool | other
  ya_make.rs              # envelope + route by mode (-t vs build)
  ya_test.rs              # `ya test` (often overlaps make -t output)
  arc_cmd.rs              # arc status/log/diff/show (smaller wins; easy)
  envelope.rs             # parse suite headers, Logsdir, totals
  detect.rs               # fingerprint inner runner
  adapters/
    py3test.rs            # strip ya framing → reuse python::pytest filter
    go_test.rs
    jest_vitest.rs
    generic_fail.rs       # fallback: keep [fail] blocks + summary only
```

Hook registration in `src/discover/rules.rs` for `^ya\s+` and selected `^arc\s+`.

Clap: `Commands::Ya { … }` / `Commands::Arc { … }` mirroring `Git` / `Cargo` sub-enum patterns in `main.rs`.

---

### Phased roadmap

#### Phase 0 — Fixtures & corpus (≈0.5–1 day)

- Export anonymized fixtures from the analytics pastes (and any `agent-tools` dumps): success, single fail, multi-fail, TIMEOUT, large chunk summaries.
- Label each fixture: outer flags (`-t` / `-tt` / `-ttX` / `-F`) + inferred inner (`py3test`, …).
- Define success metrics: **≥60% token cut** on failure dumps without dropping failing test names / assertion heads / `Logsdir` path.

#### Phase 1 — Outer envelope filter for `ya make` / `ya test` (P0)

**Goal:** compress the always-present Arcadia framing regardless of inner tool.

Keep:

- Target / suite identity
- Per-chunk one-line status when failures exist
- Each `[fail] …` headline
- Final `Total N suite` / counts (GOOD/FAIL/TIMEOUT/SKIPPED)
- `Logsdir:` (once)

Drop / collapse:

- Repeated PEERDIR / proto / compile progress when exit ≠ 0 and failures exist (or always under ultra-compact)
- Redundant separators / banner lines
- Duplicate `Logsdir` spam between chunks
- Passed-test chatter when any failure present (mirror pytest/cargo behavior)

**Implementation sketch:** streaming `BlockHandler` / state machine on suite phases (similar to `CargoTestHandler` / pytest `ParseState`).

**CLI:** `rtk ya make -- …` passthrough args; detect `-t` / `-tt*` / `--test` style flags to choose “test mode” vs “build mode”.

#### Phase 2 — Inner-runner detection & dispatch (P0 for Python)

**Fingerprint table (initial):**

| Fingerprint in output | Adapter | Reuse |
|-----------------------|---------|--------|
| `py3test`, `Logsdir:…/test-results/py3test` | `adapters/py3test` | `filter_pytest_output` (or shared core) |
| `go test`, `=== RUN`, package NDJSON if forced | `adapters/go_test` | `go_cmd` test filter |
| `FAIL  pkg`, cargo-like | `adapters/cargo_test` | `filter_cargo_test` |
| vitest/jest banners | `adapters/jest_vitest` | existing js filters |
| unknown | `adapters/generic_fail` | keep fail blocks + summary only |

**Detection order:** scan captured (or streamed) text for fingerprints; **Makefile/target type is a hint only**, never the sole signal.

Python first — matches analytics ROI.

#### Phase 3 — `ya test` parity + filter args (`-F`, `-ttX`)

- Ensure `rtk ya test …` and `rtk ya make -t …` share envelope + adapters.
- Preserve user filters (`-F '…'`) untouched in argv.
- Document that ultra-verbose modes (`-ttX`) still benefit from fail-block extraction (analytics: 7.5k tok single paste).

#### Phase 4 — Build-only `ya make` (no `-t`) (P1)

Separate path: keep errors/warnings, collapse `Ok [n/m]` progress (akin to cargo build streaming). Optional later: `ya make python -r` style proto spam (seen in sibling project dumps).

#### Phase 5 — `arc` commands (P1 / easy wins)

| Command | Strategy |
|---------|----------|
| `arc status` | Compact like `git status` |
| `arc log` | Cap commits / oneline bias like `git log` |
| `arc diff` / `arc show` | Reuse `compact_diff` patterns from git/gh |
| `arc pr` / misc | Passthrough + tracking until needed |

Lower token impact in the analytics sample, but high agent frequency.

#### Phase 6 — Hooks & discover (P1)

- Rewrite `ya make`, `ya test`, `ya tool …` (allowlist), `arc status|log|diff|show` → `rtk …`.
- **Do not** rewrite arbitrary `ya tool <anything>` until per-tool filters exist (proxy/passthrough + track).
- Document Arcadia agents: prefer `rtk ya make -t …` in AGENTS.md / project skills.

#### Phase 7 — Hardening (P2)

- Streaming mode for long multi-chunk suites (analytics: 680–1482 tests / chunk) — avoid buffering multi‑MB logs.
- Tee recovery on failure (`RunOptions::…tee("ya")`) so humans can open raw + `Logsdir`.
- Never strip `Logsdir` / log file paths for failed tests.
- Cross-platform: darwin/linux arm64/x86 fixture variants (`default-darwin-arm64-debug` strings).
- Explicit non-goals v1: rewriting RECIPE, parsing all Makefile dialects, injecting pytest flags through ya.

---

### Mapping to existing RTK patterns

| Need | Existing precedent |
|------|-------------------|
| Meta-command → subfilters | `lint_cmd` / `format_cmd` routing by ecosystem |
| Multi-phase test output | `pytest_cmd` state machine; `CargoTestHandler` |
| Streaming long builds | `run_streamed` + `BlockStreamFilter` |
| Flag-aware behavior | pytest `--tb` detection; cargo verbose guards |
| Sub-enum CLI | `GitCommands`, `GoCommands`, `DotnetCommands` |
| Hook rewrite | `discover/rules.rs` pytest / vitest / golangci patterns |

---

### Suggested priority backlog (engineering order)

1. **Fixtures** from Part 1 pastes (`py3test` multi-fail).
2. **`rtk ya make` test-mode envelope** + generic fail-block keeper (immediate win without perfect detection).
3. **`py3test` adapter → pytest filter reuse** (largest ROI).
4. Hook rules for `ya make` / `ya test`.
5. `rtk arc {status,log,diff,show}`.
6. Go / JS adapters behind the same detector.
7. Streaming + build-only `ya make`.
8. Optional: `ya tool` allowlisted filters (e.g. linters) case-by-case.

---

### Open questions (resolve during Phase 0–1)

1. Should `rtk ya` live as ecosystem `yandex/` or `arcadia/` naming?
2. Is `ya test` always the same formatter as `ya make -t` for Python targets in pay-plus, or are there RECIPE divergences worth dual fixtures?
3. For hooks: rewrite only when `ya` is on `PATH` / inside an arc mount, or always?
4. Ultra-compact policy: keep one failed assertion body vs headline-only + `Logsdir`?

---

### Success criteria (v1)

- On fixtures derived from Part 1: **≥60%** token reduction on failure dumps; **100%** retention of failed test node ids + `Logsdir`.
- Exit codes identical to bare `ya` / `arc`.
- Unknown inner runner → generic fail filter or raw fallback — never silent truncation of failures.
- Startup overhead still within RTK budget (&lt;10 ms proxy path excluding the child tool).
