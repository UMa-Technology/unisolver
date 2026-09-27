#!/usr/bin/env python3
"""Builds the C library for one platform and packs it for a GitHub release.

Usage: package_cabi.py <platform> <version> <out-dir>

  platform  macos-universal | linux-x86_64 | windows-x86_64 | windows-aarch64
  version   the release tag (v0.2.0), or any label for a dry run

Writes <out-dir>/unisolver-cabi-<version>-<platform>.zip, whose single top-level directory
holds include/unisolver.h, the dynamic and static libraries under lib/, LICENSE-MIT,
LICENSE-APACHE and THIRD_PARTY_LICENSES.md. Run it on the platform itself (macOS builds both
architectures and merges them with lipo; Windows cross-builds aarch64 on an x86_64 host).
"""
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MSVC_LIBS = ["unisolver_cabi.dll", "unisolver_cabi.dll.lib", "unisolver_cabi.lib"]
PLATFORMS = {
    "macos-universal": (["aarch64-apple-darwin", "x86_64-apple-darwin"],
                        ["libunisolver_cabi.dylib", "libunisolver_cabi.a"]),
    "linux-x86_64": (["x86_64-unknown-linux-gnu"],
                     ["libunisolver_cabi.so", "libunisolver_cabi.a"]),
    "windows-x86_64": (["x86_64-pc-windows-msvc"], MSVC_LIBS),
    "windows-aarch64": (["aarch64-pc-windows-msvc"], MSVC_LIBS),
}
DOCS = ["LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_LICENSES.md"]
HEADER = ROOT / "crates/unisolver-cabi/include/unisolver.h"
INSTALL_NAME = "@rpath/libunisolver_cabi.dylib"


def run(*cmd):
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in PLATFORMS:
        sys.exit(__doc__)
    platform, version, out = sys.argv[1], sys.argv[2], Path(sys.argv[3]).resolve()
    targets, libs = PLATFORMS[platform]
    for t in targets:
        run("rustup", "target", "add", t)
        run("cargo", "build", "-p", "unisolver-cabi", "--release", "--target", t)

    name = f"unisolver-cabi-{version}-{platform}"
    with tempfile.TemporaryDirectory() as tmp:
        stage = Path(tmp) / name
        (stage / "lib").mkdir(parents=True)
        (stage / "include").mkdir()
        for lib in libs:
            built = [str(ROOT / "target" / t / "release" / lib) for t in targets]
            if len(built) > 1:
                run("lipo", "-create", "-output", str(stage / "lib" / lib), *built)
            else:
                shutil.copy2(built[0], stage / "lib" / lib)
        if platform.startswith("macos"):
            dylib = str(stage / "lib" / "libunisolver_cabi.dylib")
            ident = subprocess.run(["otool", "-D", dylib], check=True, capture_output=True,
                                   text=True).stdout.splitlines()[-1].strip()
            if ident != INSTALL_NAME:
                sys.exit(f"{dylib}: install name {ident}, expected {INSTALL_NAME}")
        shutil.copy2(HEADER, stage / "include")
        for doc in DOCS:
            shutil.copy2(ROOT / doc, stage)

        out.mkdir(parents=True, exist_ok=True)
        archive = out / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted(p for p in stage.rglob("*") if p.is_file()):
                z.write(f, f.relative_to(stage.parent))
    print(f"wrote {archive} ({archive.stat().st_size // 1024} KiB)")


if __name__ == "__main__":
    main()
