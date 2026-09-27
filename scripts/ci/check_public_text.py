#!/usr/bin/env python3
"""Gate on text in the public tree (everything tracked outside internal/).

1. CJK characters may appear only where scripts/ci/cjk-allowlist.txt allows:
   `file <glob>` allows a whole file (localized data); `literals <glob>`
   allows CJK only inside string literals or Markdown code spans (localized
   test expectations, built-in name tables, examples in docs); `line <glob>
   <text>` allows one exact line. Everything else must be CJK-free.
2. If internal/denylist.txt exists, no public file may contain any of its
   terms (case-insensitive). The list is kept internal on purpose: a public
   list would itself publish the terms.

Usage: check_public_text.py [--list]
  --list  print per-file counts of lines containing CJK and exit 0
"""
import fnmatch
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CJK = re.compile(r"[\u3000-\u303f\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff00-\uffef]")
STRING = re.compile(r'"(?:[^"\\]|\\.)*"' + r"|'(?:[^'\\]|\\.)*'" + r"|`[^`]*`")
BINARY_EXT = {".png", ".jpg", ".jpeg", ".bin", ".zst", ".db", ".fits", ".ico",
              ".icns", ".pyc", ".jar", ".webp", ".gif"}


def tracked_public_files():
    out = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, check=True,
                         capture_output=True).stdout
    for name in out.decode().split("\0"):
        if name and not name.startswith("internal/"):
            yield name


def read_text(path):
    p = ROOT / path
    if p.suffix.lower() in BINARY_EXT or not p.is_file() or p.is_symlink():
        return None
    data = p.read_bytes()
    if b"\0" in data[:8192]:
        return None
    return data.decode("utf-8", errors="replace")


def load_allowlist():
    rules, exact = [], set()
    for line in (ROOT / "scripts/ci/cjk-allowlist.txt").read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        mode, rest = line.split(None, 1)
        if mode == "line":
            glob, text = rest.split(None, 1)
            exact.add((glob, text))
        elif mode in ("file", "literals"):
            rules.append((mode, rest))
        else:
            sys.exit(f"cjk-allowlist.txt: unknown mode in '{line}'")
    return rules, exact


def mode_for(path, rules):
    for mode, glob in rules:
        if fnmatch.fnmatch(path, glob):
            return mode
    return None


def load_denylist():
    f = ROOT / "internal/denylist.txt"
    if not f.is_file():
        return None
    return [t.strip().lower() for t in f.read_text(encoding="utf-8").splitlines()
            if t.strip() and not t.lstrip().startswith("#")]


def main():
    list_only = "--list" in sys.argv[1:]
    rules, exact = load_allowlist()
    deny = load_denylist()
    problems, counts = [], {}
    for path in tracked_public_files():
        text = read_text(path)
        if text is None:
            continue
        mode = mode_for(path, rules)
        for n, line in enumerate(text.splitlines(), 1):
            if CJK.search(line):
                counts[path] = counts.get(path, 0) + 1
                if any(fnmatch.fnmatch(path, g) and line.strip() == t for g, t in exact):
                    continue
                if mode != "file":
                    rest = STRING.sub("", line) if mode == "literals" else line
                    if CJK.search(rest):
                        problems.append(f"{path}:{n}: CJK outside the allowlist")
            if deny:
                low = line.lower()
                for term in deny:
                    if term in low:
                        problems.append(f"{path}:{n}: internal term '{term}'")
    if list_only:
        for path, c in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])):
            print(f"{c:5} {path}")
        print(f"{sum(counts.values())} lines in {len(counts)} files")
        return 0
    if deny is None:
        print("denylist not available (internal/denylist.txt absent): skipped that check")
    for p in problems:
        print(p)
    print(f"{len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
