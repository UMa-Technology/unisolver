#!/usr/bin/env python3
"""Lays out tier archives by manifest key, the way a host serves them, behind a static
server **with Range support**, for rehearsing DbManager's download path locally.

    python3 scripts/ci/serve_tiers.py --manifest M --data DIR          # port 8099, every tier
    python3 scripts/ci/serve_tiers.py ... --only <tier name>           # a single tier
    python3 scripts/ci/serve_tiers.py ... --truncate-first 4000000     # drop the first archive at 4 MB

Why not `python3 -m http.server`: stdlib's SimpleHTTPRequestHandler ignores Range and
answers 200 with the full body, so the resume path would silently degrade to a restart
and never be exercised. This server answers 206 with Content-Range, like object storage
and CDNs do.

The layout uses symlinks rather than copies (a tier can exceed 1 GB). The manifest is
served unchanged: clients only point their base URL here, and keys and sha256 come from
the manifest, so the rehearsed manifest is the one that gets published.
"""
from __future__ import annotations

import argparse
import http.server
import json
import os
import re
import shutil
import tempfile
from pathlib import Path


class RangeHandler(http.server.SimpleHTTPRequestHandler):
    """SimpleHTTPRequestHandler + `Range: bytes=N-` (206/416) + an optional early disconnect."""

    truncate_first: int | None = None
    _truncated_once = False

    def log_message(self, fmt: str, *args) -> None:  # compact log: one line per request
        rng = self.headers.get("Range")
        print(f"  {self.command} {self.path}{' ' + rng if rng else ''} -> {args[1]}")

    def send_head(self):  # noqa: C901 - mirrors stdlib's structure for easy comparison
        rng = self.headers.get("Range")
        if not rng:
            return super().send_head()
        path = self.translate_path(self.path)
        if not os.path.isfile(path):
            self.send_error(404, "File not found")
            return None
        m = re.fullmatch(r"bytes=(\d+)-(\d*)", rng.strip())
        size = os.path.getsize(path)
        if not m:
            self.send_error(400, "Malformed Range")
            return None
        start = int(m.group(1))
        end = int(m.group(2)) if m.group(2) else size - 1
        if start >= size:
            self.send_response(416)
            self.send_header("Content-Range", f"bytes */{size}")
            self.end_headers()
            return None
        f = open(path, "rb")
        f.seek(start)
        self.send_response(206)
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        return f

    def copyfile(self, source, outputfile):
        cut = type(self).truncate_first
        if cut is not None and not type(self)._truncated_once and self.path.endswith(".zst"):
            type(self)._truncated_once = True
            print(f"  ** sending only {cut} bytes, then disconnecting (resume test)")
            outputfile.write(source.read(cut))
            return
        shutil.copyfileobj(source, outputfile)


def build_layout(manifest: Path, data: Path, only: list[str]) -> Path:
    m = json.loads(manifest.read_text())
    root = Path(tempfile.mkdtemp(prefix="unisolver_cdn_"))
    (root / "manifest.json").symlink_to(manifest.resolve())
    served = []
    for t in m["tiers"]:
        if only and t["name"] not in only:
            continue
        if t.get("bundled"):
            continue  # the bundled tier is not hosted
        src = (data / t["file"]).resolve()
        if not src.is_file():
            print(f"  skipping {t['name']}: {src} does not exist")
            continue
        dst = root / t["key"]
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.symlink_to(src)
        served.append((t["name"], t["key"], t["bytes"]))
    if not served:
        raise SystemExit("nothing to serve: check --data and --only")
    print(f"layout: {root}")
    for name, key, b in served:
        print(f"  {name:20s} {b / 1e6:8.1f} MB  /{key}")
    return root


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", required=True)
    ap.add_argument("--data", required=True)
    ap.add_argument("--port", type=int, default=8099)
    ap.add_argument("--only", action="append", default=[], help="serve only these tiers (repeatable)")
    ap.add_argument(
        "--truncate-first",
        type=int,
        help="send only this many bytes of the first .zst request, then disconnect (resume test)",
    )
    a = ap.parse_args()
    root = build_layout(Path(a.manifest), Path(a.data), a.only)
    RangeHandler.truncate_first = a.truncate_first
    os.chdir(root)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", a.port), RangeHandler)
    print(f"serving http://127.0.0.1:{a.port}/ (Ctrl-C to stop)")
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
