#!/usr/bin/env python3
"""Refuses agent guidance and docs that name a repository path or test that no longer exists.

Checks every backticked path whose first segment is a top-level entry of the repository, and
every `cargo test -p <crate> --test <binary> <module>::<name>` filter, in AGENTS.md,
CONTRIBUTING.md, CODING_STANDARDS.md, docs/ and .claude/skills/.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIPPED = ("docs/measurements/", "docs/adr/", "docs/acceptance-", "docs/research/")
SPAN = re.compile(r"`([^`\n]+)`")
PATH = re.compile(r"^\.?[\w.-]+(/[\w.@-]+)*/?$")
FILTER = re.compile(r"cargo test\b[^`\n]*?-p (\S+)[^`\n]*?--test (\w+) ([\w:]+)")


def documents():
    for name in ("AGENTS.md", "CONTRIBUTING.md", "CODING_STANDARDS.md"):
        if (ROOT / name).exists():
            yield ROOT / name
    for folder in ("docs", ".claude/skills"):
        for path in sorted((ROOT / folder).rglob("*.md")):
            relative = path.relative_to(ROOT).as_posix()
            if not relative.startswith(SKIPPED):
                yield path


def missing_path(span, top):
    candidate = re.sub(r"(:\d+(-\d+)?|::\w+)+$", "", span.split()[0])
    if not PATH.match(candidate) or "/" not in candidate.rstrip("/"):
        return None
    if candidate.split("/")[0] not in top or candidate.startswith(".kestrel/"):
        return None
    return None if (ROOT / candidate).exists() else candidate


def missing_test(crate, binary, filter):
    tests = ROOT / "crates" / crate / "tests"
    module, _, name = filter.partition("::")
    if "::" in name or not name:
        return None
    sources = [tests / binary / f"{module}.rs", *sorted((tests / binary / module).rglob("*.rs"))]
    if binary == module:
        sources.append(tests / f"{binary}.rs")
    texts = [source.read_text() for source in sources if source.exists()]
    if not texts:
        return f"--test {binary} {filter}: no module {module}"
    if not any(re.search(rf"\bfn {re.escape(name)}", text) for text in texts):
        return f"--test {binary} {filter}: no test starting {name}"
    return None


def main():
    top = {entry.name for entry in ROOT.iterdir()}
    problems = []
    for document in documents():
        for number, line in enumerate(document.read_text().splitlines(), 1):
            found = [missing_path(span, top) for span in SPAN.findall(line)]
            found += [missing_test(*match) for match in FILTER.findall(line)]
            where = f"{document.relative_to(ROOT)}:{number}"
            problems += [f"{where}: {problem}" for problem in found if problem]
    for problem in problems:
        print(problem)
    if problems:
        print(
            f"\n{len(problems)} reference(s) name a path or test that does not exist. Point each "
            "at where the thing lives now, or delete the sentence if it no longer applies.",
            file=sys.stderr,
        )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
