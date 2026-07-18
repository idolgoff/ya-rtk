# `ya` fixtures

Real Arcadia **`ya make` / `ya test` / `ya tool`** CLI outputs for RTK filters.
Includes **pass / fail / build / mixed** — not only failures.

Inclusion requires strong fingerprints (`ya make`, `py3test`, `go_test`, `Logsdir`,
`chunk ran`, `[TM]`, `[PB]`, etc.). Source dumps and raw transcripts are excluded.

## Golden set

CI-critical fixtures (G1–G10), compact policy, keep/drop rules, and min savings are in
**[ACCEPTANCE.md](ACCEPTANCE.md)**.

All other files in this directory are **corpus / regression** — useful for broader coverage,
not required for the Stage 2+ acceptance gate.

## Workspace coverage

| Workspace | Notes |
|-----------|-------|
| yandex-pay-plus | python — many `ya make -t` / py3test dumps |
| yandex-pay-admin | python — `ya make -t`, `ya make python -r` |
| receiptron | go — `ya make -t` / `go_test` |
| frontend-pay | ts — almost no `ya` pastes in Cursor transcripts |
| pay-console | ts — **empty `agent-transcripts/`** in workspace project dir |

Related `agent-tools` under `arcadia-*` folders were also scanned.

Paths: `/Users/idolgoff/` → `/Users/user/`.

**Total: 54 files** (849 KB).

## Counts

- `py/fail`: 32
- `go/fail`: 8
- `py/other`: 4
- `py/pass_or_mixed`: 3
- `unk/fail`: 2
- `go/other`: 2
- `unk/other`: 2
- `py/build`: 1

| File | Bytes |
|------|------:|
| `make_t_py_fail_large_logsdir_chunk_slice_raw.txt` | 130,054 |
| `make_python_py_build_large_proto_raw.txt` | 125,871 |
| `make_python_py_other_large_raw.txt` | 125,386 |
| `make_t_py_fail_large_logsdir_chunk_raw.txt` | 89,461 |
| `make_t_py_fail_large_logsdir_chunk_a272a54e3332.txt` | 64,939 |
| `make_t_py_fail_large_logsdir_raw.txt` | 43,576 |
| `make_ttX_py_fail_logsdir_chunk_raw.txt` | 31,827 |
| `make_t_py_fail_logsdir_chunk_raw.txt` | 30,292 |
| `make_t_py_fail_logsdir_chunk_538df13cb42d.txt` | 28,615 |
| `make_t_py_fail_logsdir_chunk_55cdce2e28ae.txt` | 20,323 |
| `make_t_py_fail_logsdir_raw.txt` | 15,210 |
| `make_t_py_fail_logsdir_chunk_9980e5f3094d.txt` | 14,168 |
| `make_t_py_fail_logsdir_chunk_5b91158d5d77.txt` | 12,098 |
| `make_t_py_pass_or_mixed_raw.txt` | 10,504 |
| `make_t_py_pass_or_mixed_25aad01e565f.txt` | 10,502 |
| `make_t_py_pass_or_mixed_logsdir_chunk_raw.txt` | 9,484 |
| `tool_unk_other_raw.txt` | 6,459 |
| `make_t_go_fail_logsdir_chunk_raw.txt` | 5,963 |
| `make_t_py_fail_logsdir_chunk_6eb82b5c5dee.txt` | 5,806 |
| `make_t_py_fail_logsdir_chunk_06cc770220a6.txt` | 5,764 |
| `make_t_py_other_raw.txt` | 5,370 |
| `make_t_py_other_logsdir_raw.txt` | 4,916 |
| `make_t_py_fail_logsdir_chunk_7039d88c7363.txt` | 4,581 |
| `make_tt_go_fail_logsdir_chunk_raw.txt` | 4,430 |
| `make_t_py_fail_logsdir_62fcda3403a5.txt` | 3,864 |
| `make_t_py_fail_logsdir_fa5e2fa3f402.txt` | 3,787 |
| `make_t_py_fail_logsdir_1c0967fa1574.txt` | 3,783 |
| `make_t_py_fail_logsdir_chunk_aed278af16f3.txt` | 3,171 |
| `make_t_go_fail_logsdir_chunk_9aec11fae0b0.txt` | 3,169 |
| `make_t_go_fail_logsdir_chunk_86af57e191c5.txt` | 3,136 |
| `make_t_go_fail_logsdir_chunk_15cfbe8d2726.txt` | 3,036 |
| `make_t_py_fail_logsdir_9abadcccdc67.txt` | 2,960 |
| `make_t_py_fail_logsdir_358903fa5616.txt` | 2,935 |
| `make_t_go_fail_logsdir_chunk_0ff44b95984f.txt` | 2,906 |
| `make_t_go_fail_logsdir_chunk_6e97d54841af.txt` | 2,886 |
| `make_t_go_fail_logsdir_chunk_a2ec065091d8.txt` | 2,466 |
| `make_t_py_fail_logsdir_chunk_8fd5535c6064.txt` | 2,149 |
| `make_t_py_fail_logsdir_chunk_c7c4f7ea71b7.txt` | 1,855 |
| `make_t_py_fail_logsdir_chunk_fa932db923cd.txt` | 1,722 |
| `make_t_py_fail_logsdir_3d01261eec93.txt` | 1,677 |
| `make_tt_go_other_raw.txt` | 1,562 |
| `make_t_py_other_logsdir_6d69456a7337.txt` | 1,521 |
| `make_t_py_fail_logsdir_chunk_98467e6f7180.txt` | 1,479 |
| `make_t_py_fail_logsdir_059a8484376d.txt` | 1,478 |
| `make_t_py_fail_logsdir_chunk_f69d988ca649.txt` | 1,459 |
| `make_t_py_fail_logsdir_chunk_77ba0f7d78f0.txt` | 1,446 |
| `make_t_unk_fail_raw.txt` | 1,436 |
| `make_t_py_fail_logsdir_d3a718b1282c.txt` | 1,374 |
| `make_t_py_fail_logsdir_chunk_0029c2806563.txt` | 1,299 |
| `make_t_py_fail_logsdir_09a072ce09a8.txt` | 1,180 |
| `make_unk_fail_raw.txt` | 1,148 |
| `make_tt_go_other_def2ba3c83b6.txt` | 1,088 |
| `make_t_py_fail_logsdir_8f0165aa37a0.txt` | 854 |
| `tool_unk_other_a794aa6341cd.txt` | 782 |
