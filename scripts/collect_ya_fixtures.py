#!/usr/bin/env python3
"""Collect real ya/Arcadia CLI outputs into tests/fixtures/ya/.

Only keeps dumps that look like ya make/test/tool output (strong fingerprints).
Includes pass/fail/build — not failures only.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
from collections import Counter
from pathlib import Path

PROJECTS = Path("/Users/idolgoff/.cursor/projects")
OUT = Path(__file__).resolve().parents[1] / "tests" / "fixtures" / "ya"

WORKSPACES = [
    "Users-idolgoff-workspace-frontend-pay-frontend-pay-code-workspace",
    "Users-idolgoff-workspace-pay-console-pay-console-code-workspace",
    "Users-idolgoff-workspace-receiptron-receiptron-code-workspace",
    "Users-idolgoff-workspace-vscode-yandex-pay-admin-yandex-pay-admin-code-workspace",
    "Users-idolgoff-workspace-yandex-pay-plus-yandex-pay-plus-code-workspace",
]

RELATED = [
    "Users-idolgoff-arcadia-billing-yandex-pay-admin",
    "Users-idolgoff-arcadia-billing-yandex-pay-admin-yandex-pay-admin",
    "Users-idolgoff-arcadia-billing-yandex-pay-plus",
    "Users-idolgoff-arcadia-passport-frontend-id-services-pay-console",
    "Users-idolgoff-arcadia-pay-frontend-services-pay",
    "Users-idolgoff-arcadia-pay-receiptron",
    "Users-idolgoff-arcadia-pay-lib",
]

MIN_CHARS = 600
MAX_STORE = 150_000

TERM_SEL_RE = re.compile(r"<terminal_selection[^>]*>(.*?)</terminal_selection>", re.S)
CODE_SEL_RE = re.compile(r"<code_selection([^>]*)>(.*?)</code_selection>", re.S)

# Must match — real ya orchestration / test runner framing
STRONG = re.compile(
    r"(?:"
    r"\bya\s+make\b|\bya\s+test\b|\bya\s+tool\b|"
    r"<py3test>|<go_test>|<gtest>|py3test\s*>>|"
    r"Logsdir:\s*\S*test-results|"
    r"sole chunk ran|chunk ran \d+ tests|"
    r"Total \d+ suite|"
    r"\[TM\]\s*\{|"
    r"Ok\s+\[[\d/]+\]|"
    r"\[PB\]\s*\$\(B\)|"
    r"Y_PYTHON_SOURCE_ROOT|"
    r"Test command err:|"
    r"gotest\s*<go_test>"
    r")",
    re.I,
)

JUNK_HEAD = re.compile(
    r"(?:"
    r'^\{"role":"(user|assistant)"|'
    r"<manually_attached_skills>|"
    r"^---\s*\nname:\s|"
    r"^L\d+:---"
    r")",
    re.M,
)


def sanitize(text: str) -> str:
    text = text.replace("/Users/idolgoff/", "/Users/user/")
    text = re.sub(
        r"/Users/user/\.cursor/projects/[^\s\"']+",
        "/Users/user/.cursor/projects/…",
        text,
    )
    return text


def extract_cmds(text: str) -> list[str]:
    return re.findall(r"ya\s+(?:make|test|tool|package)[^\n`]{0,140}", text)


def is_real_ya_output(body: str) -> bool:
    if len(body) < MIN_CHARS:
        return False
    if JUNK_HEAD.search(body[:1500]):
        return False
    # reject pure path listings / code-search dumps without ya framing
    if not STRONG.search(body[:12000]) and not STRONG.search(body[-6000:]):
        return False
    # reject agent-tools that are mostly "path:line: code" without suite framing
    pathline = len(re.findall(r"^/?(?:Users|arcadia|pay|billing|modules)/[\w./-]+\.\w+:\d+:", body[:4000], re.M))
    if pathline >= 15 and not re.search(r"chunk ran|Total \d+ suite|Logsdir:|\[TM\]|ya make", body[:8000]):
        return False
    return True


def detect_lang(body: str) -> str:
    if re.search(r"py3test|<py3test>|Y_PYTHON_|pytest", body):
        return "py"
    if re.search(r"go_test|<go_test>|=== RUN|gotest", body):
        return "go"
    if re.search(r"hermione|jest|vitest|mocha|playwright", body, re.I):
        return "ts"
    if re.search(r"gtest|<gtest>", body):
        return "cpp"
    return "unk"


def detect_outcome(body: str) -> str:
    if re.search(r"\[fail\]|\bFAILED\b|\bFAIL:\b|Total \d+ suite:.*FAIL", body):
        return "fail"
    if re.search(r"\bTIMEOUT\b", body):
        return "timeout"
    if re.search(r"Total \d+ suite:.*GOOD|Total \d+ suite: \d+ - GOOD", body):
        return "pass"
    if re.search(r"\[PB\]|Ok \[|Compiling|Linking", body) and not re.search(r"chunk ran|\[fail\]", body):
        return "build"
    if re.search(r"chunk ran|Total \d+ suite", body):
        return "pass_or_mixed"
    return "other"


def detect_fam(body: str, cmds: list[str]) -> str:
    blob = (" ".join(cmds) + "\n" + body[:2500]).lower()
    if "ya make python" in blob:
        return "make_python"
    if "ya make -ttx" in blob:
        return "make_ttX"
    if re.search(r"ya make -tt\b", blob):
        return "make_tt"
    if re.search(r"ya make -t\b", blob) or re.search(r"py3test|go_test|Logsdir:", body[:5000]):
        return "make_t"
    if re.search(r"\bya test\b", blob):
        return "test"
    if re.search(r"\bya make\b", blob) or re.search(r"\[PB\]|Ok \[", body):
        return "make"
    if re.search(r"\bya tool\b", blob):
        return "tool"
    return "ya"


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    # Wipe and rebuild corpus cleanly
    for p in OUT.glob("*.txt"):
        p.unlink()

    items: list[dict] = []
    seen: set[str] = set()
    stats: Counter = Counter()

    def add_item(body: str, source: str, workspace: str, cmds: list[str] | None = None) -> None:
        cmds = cmds or []
        if not is_real_ya_output(body):
            stats["reject"] += 1
            return
        san = sanitize(body)
        h = hashlib.sha256(san.encode()).hexdigest()
        if h in seen:
            stats["dup"] += 1
            return
        seen.add(h)
        lang = detect_lang(body)
        outcome = detect_outcome(body)
        fam = detect_fam(body, cmds)
        items.append(
            {
                "body": san,
                "chars": len(body),
                "hash": h[:12],
                "source": source,
                "workspace": workspace,
                "cmds": cmds[:4],
                "lang": lang,
                "outcome": outcome,
                "fam": fam,
            }
        )
        stats[f"{lang}/{outcome}"] += 1
        stats[workspace] += 1

    def scan_transcripts(base: Path, workspace: str) -> None:
        tdir = base / "agent-transcripts"
        if not tdir.is_dir():
            return
        for root, _, files in os.walk(tdir):
            for f in files:
                if not f.endswith(".jsonl"):
                    continue
                with open(Path(root) / f, errors="ignore") as fh:
                    for line in fh:
                        try:
                            obj = json.loads(line)
                        except Exception:
                            continue
                        content = (obj.get("message") or {}).get("content")
                        texts: list[str] = []
                        if isinstance(content, list):
                            texts = [c.get("text", "") for c in content if isinstance(c, dict)]
                        elif isinstance(content, str):
                            texts = [content]
                        for text in texts:
                            cmds = extract_cmds(text[:4000])
                            for m in TERM_SEL_RE.finditer(text):
                                add_item(m.group(1), "terminal_selection", workspace, cmds)
                            for m in CODE_SEL_RE.finditer(text):
                                attrs, body = m.group(1), m.group(2)
                                if "test-results" in attrs or STRONG.search(body[:5000]):
                                    add_item(body, "code_selection", workspace, cmds)

    def scan_agent_tools(base: Path, workspace: str) -> None:
        tdir = base / "agent-tools"
        if not tdir.is_dir():
            return
        for fp in tdir.iterdir():
            if not fp.is_file():
                continue
            try:
                body = fp.read_text(errors="ignore")
            except Exception:
                continue
            add_item(body, f"agent-tools:{fp.name[:8]}", workspace, extract_cmds(body[:3000]))

    def scan_terminals(base: Path, workspace: str) -> None:
        tdir = base / "terminals"
        if not tdir.is_dir():
            return
        for fp in tdir.iterdir():
            if not fp.is_file() or fp.suffix != ".txt":
                continue
            try:
                body = fp.read_text(errors="ignore")
            except Exception:
                continue
            if body.startswith("---"):
                parts = body.split("---", 2)
                if len(parts) >= 3:
                    body = parts[2]
            add_item(body, f"terminal:{fp.name}", workspace, extract_cmds(body))

    for name in WORKSPACES + RELATED:
        base = PROJECTS / name
        if not base.is_dir():
            print(f"MISSING {name}")
            continue
        print(f"Scanning {name}")
        scan_transcripts(base, name)
        scan_agent_tools(base, name)
        scan_terminals(base, name)

    used: set[str] = set()
    for it in sorted(items, key=lambda x: -x["chars"]):
        slug = f"{it['fam']}_{it['lang']}_{it['outcome']}"
        if it["chars"] >= 40_000:
            slug += "_large"
        if "Logsdir:" in it["body"]:
            slug += "_logsdir"
        if "chunk ran" in it["body"]:
            slug += "_chunk"
        if re.search(r"\[PB\]", it["body"]):
            slug += "_proto"

        body = it["body"]
        if len(body) > MAX_STORE:
            body = (
                body[:80_000]
                + f"\n\n… [FIXTURE TRUNCATED: original {it['chars']} chars] …\n\n"
                + body[-50_000:]
            )
            slug += "_slice"

        name = f"{slug}_raw.txt"
        if name in used:
            name = f"{slug}_{it['hash']}.txt"
        used.add(name)
        (OUT / name).write_text(body)
        print(f"+ {name:70s} {it['chars']:7,}  {it['lang']}/{it['outcome']}  …{it['workspace'][-40:]}")

    files = sorted(OUT.glob("*.txt"), key=lambda p: -p.stat().st_size)
    lines = [
        "# `ya` fixtures",
        "",
        "Real Arcadia **`ya make` / `ya test` / `ya tool`** CLI outputs for RTK filters.",
        "Includes **pass / fail / build / mixed** — not only failures.",
        "",
        "Inclusion requires strong fingerprints (`ya make`, `py3test`, `go_test`, `Logsdir`,",
        "`chunk ran`, `[TM]`, `[PB]`, etc.). Source dumps and raw transcripts are excluded.",
        "",
        "## Workspace coverage",
        "",
        "| Workspace | Notes |",
        "|-----------|-------|",
        "| yandex-pay-plus | python — many `ya make -t` / py3test dumps |",
        "| yandex-pay-admin | python — `ya make -t`, `ya make python -r` |",
        "| receiptron | go — `ya make -t` / `go_test` |",
        "| frontend-pay | ts — almost no `ya` pastes in Cursor transcripts |",
        "| pay-console | ts — **empty `agent-transcripts/`** in workspace project dir |",
        "",
        "Related `agent-tools` under `arcadia-*` folders were also scanned.",
        "",
        "Paths: `/Users/idolgoff/` → `/Users/user/`.",
        "",
        f"**Total: {len(files)} files** ({sum(p.stat().st_size for p in files)/1024:.0f} KB).",
        "",
        "## Counts",
        "",
    ]
    for k, v in sorted(
        ((k, v) for k, v in stats.items() if "/" in k),
        key=lambda kv: -kv[1],
    ):
        lines.append(f"- `{k}`: {v}")
    lines += ["", "| File | Bytes |", "|------|------:|"]
    for p in files:
        lines.append(f"| `{p.name}` | {p.stat().st_size:,} |")
    (OUT / "README.md").write_text("\n".join(lines) + "\n")

    print("\nStats (lang/outcome):", {k: v for k, v in stats.items() if "/" in k})
    print(f"reject={stats['reject']} dup={stats['dup']}")
    print(f"Total fixtures: {len(files)}")
    print(f"Disk: {(sum(p.stat().st_size for p in files)/1024):.0f} KB")


if __name__ == "__main__":
    main()
