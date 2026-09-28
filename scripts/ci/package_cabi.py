#!/usr/bin/env python3
"""Builds the C library for one platform and packs it for a GitHub release.

Usage: package_cabi.py <platform> <version> <out-dir>

  platform  macos-universal | linux-x86_64 | windows-x86_64 | windows-aarch64 | ios | android
  version   the release tag (v0.2.0), or any label for a dry run

Writes <out-dir>/unisolver-cabi-<version>-<platform>.zip, whose single top-level directory
holds include/unisolver.h, the libraries, LICENSE-MIT, LICENSE-APACHE and
THIRD_PARTY_LICENSES.md. Desktop platforms carry the dynamic and static libraries under lib/.
Run it on the platform itself (macOS builds both architectures and merges them with lipo;
Windows cross-builds aarch64 on an x86_64 host). The mobile packages are for apps that do
not use the Flutter plugin, and each links a small program against what it ships:

  ios      on macOS with Xcode: unisolver.xcframework, the static library for devices (arm64)
           and simulators (arm64 + x86_64), iOS 12 or later; its headers carry a module map,
           so Swift can `import unisolver`.
  android  with the NDK (ANDROID_NDK_HOME, ANDROID_NDK_ROOT, ANDROID_NDK_LATEST_HOME, or the
           newest under $ANDROID_HOME/ndk): lib/<abi>/libunisolver_cabi.so for arm64-v8a and
           x86_64, API 21 or later, with a SONAME and 16 KB page alignment.
"""
import os
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
MOBILE = ["ios", "android"]
IOS_TARGETS = ["aarch64-apple-ios", "aarch64-apple-ios-sim", "x86_64-apple-ios"]
IOS_MIN = "12.0"
ANDROID_ABIS = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}
ANDROID_API = 21
PAGE_16K = 0x4000
DOCS = ["LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_LICENSES.md"]
HEADER = ROOT / "crates/unisolver-cabi/include/unisolver.h"
INSTALL_NAME = "@rpath/libunisolver_cabi.dylib"
MODULE_MAP = """module unisolver {
    header "unisolver.h"
    export *
}
"""
SMOKE = """#include <stdio.h>
#include "unisolver.h"

int main(void) {
    puts(unisolver_version());
    return 0;
}
"""


def run(*cmd, env=None, capture=False):
    print("+", " ".join(cmd), flush=True)
    r = subprocess.run(cmd, cwd=ROOT, check=True, env={**os.environ, **(env or {})},
                       capture_output=capture, text=capture)
    return r.stdout if capture else None


def build_lib(target, crate_type, env=None):
    """One crate type of the C library for `target` (cargo rustc overrides the crate's list)"""
    run("rustup", "target", "add", target)
    run("cargo", "rustc", "-p", "unisolver-cabi", "--release", "--target", target,
        "--crate-type", crate_type, env=env)


def build_ios(stage, tmp):
    env = {"IPHONEOS_DEPLOYMENT_TARGET": IOS_MIN}
    for t in IOS_TARGETS:
        build_lib(t, "staticlib", env)
    built = {t: str(ROOT / "target" / t / "release" / "libunisolver_cabi.a") for t in IOS_TARGETS}
    sim = tmp / "sim" / "libunisolver_cabi.a"
    sim.parent.mkdir()
    run("lipo", "-create", "-output", str(sim), built["aarch64-apple-ios-sim"],
        built["x86_64-apple-ios"])
    headers = tmp / "headers"
    headers.mkdir()
    shutil.copy2(HEADER, headers)
    (headers / "module.modulemap").write_text(MODULE_MAP)
    run("xcodebuild", "-create-xcframework",
        "-library", built["aarch64-apple-ios"], "-headers", str(headers),
        "-library", str(sim), "-headers", str(headers),
        "-output", str(stage / "unisolver.xcframework"))
    smoke = tmp / "smoke.c"
    smoke.write_text(SMOKE)
    links = [("iphoneos", f"arm64-apple-ios{IOS_MIN}", built["aarch64-apple-ios"]),
             ("iphonesimulator", f"arm64-apple-ios{IOS_MIN}-simulator", str(sim)),
             ("iphonesimulator", f"x86_64-apple-ios{IOS_MIN}-simulator", str(sim))]
    for sdk, target, lib in links:
        run("xcrun", "-sdk", sdk, "clang", "-target", target, "-I", str(headers), str(smoke),
            lib, "-o", str(tmp / f"smoke-{target}"))


def android_ndk():
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK_LATEST_HOME"]:
        if os.environ.get(var) and Path(os.environ[var]).is_dir():
            return Path(os.environ[var])
    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or str(
        Path.home() / ("Library/Android/sdk" if sys.platform == "darwin" else "Android/Sdk"))
    ndks = sorted((p for p in (Path(sdk) / "ndk").glob("*")
                   if p.name.replace(".", "").isdigit()),
                  key=lambda p: [int(x) for x in p.name.split(".")])
    if not ndks:
        sys.exit("no Android NDK: set ANDROID_NDK_HOME")
    return ndks[-1]


def build_android(stage, tmp):
    host = "darwin-x86_64" if sys.platform == "darwin" else "linux-x86_64"
    bin_dir = android_ndk() / "toolchains" / "llvm" / "prebuilt" / host / "bin"
    smoke = tmp / "smoke.c"
    smoke.write_text(SMOKE)
    for abi, t in ANDROID_ABIS.items():
        clang = str(bin_dir / f"{t}{ANDROID_API}-clang")
        key = t.upper().replace("-", "_")
        build_lib(t, "cdylib", {
            f"CARGO_TARGET_{key}_LINKER": clang,
            f"CARGO_TARGET_{key}_RUSTFLAGS": "-C link-arg=-Wl,-z,max-page-size=16384 "
                                             "-C link-arg=-Wl,-soname,libunisolver_cabi.so",
        })
        so = stage / "lib" / abi / "libunisolver_cabi.so"
        so.parent.mkdir(parents=True)
        shutil.copy2(ROOT / "target" / t / "release" / "libunisolver_cabi.so", so)
        readelf = str(bin_dir / "llvm-readelf")
        loads = [line.split() for line in run(readelf, "-lW", str(so), capture=True).splitlines()
                 if line.strip().startswith("LOAD")]
        if not loads or any(int(f[-1], 16) < PAGE_16K for f in loads):
            sys.exit(f"{so}: LOAD segments not 16 KB aligned")
        if "(SONAME)" not in run(readelf, "-dW", str(so), capture=True):
            sys.exit(f"{so}: no SONAME")
        run(clang, "-I", str(HEADER.parent), str(smoke), "-L", str(so.parent), "-lunisolver_cabi",
            "-o", str(tmp / f"smoke-{abi}"))


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in [*PLATFORMS, *MOBILE]:
        sys.exit(__doc__)
    platform, version, out = sys.argv[1], sys.argv[2], Path(sys.argv[3]).resolve()
    name = f"unisolver-cabi-{version}-{platform}"
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        stage = tmp / name
        (stage / "include").mkdir(parents=True)
        if platform == "ios":
            build_ios(stage, tmp)
        elif platform == "android":
            build_android(stage, tmp)
        else:
            build_desktop(platform, stage)
        shutil.copy2(HEADER, stage / "include")
        for doc in DOCS:
            shutil.copy2(ROOT / doc, stage)

        out.mkdir(parents=True, exist_ok=True)
        archive = out / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted(p for p in stage.rglob("*") if p.is_file()):
                z.write(f, f.relative_to(stage.parent))
    print(f"wrote {archive} ({archive.stat().st_size // 1024} KiB)")


def build_desktop(platform, stage):
    targets, libs = PLATFORMS[platform]
    for t in targets:
        run("rustup", "target", "add", t)
        run("cargo", "build", "-p", "unisolver-cabi", "--release", "--target", t)
    (stage / "lib").mkdir()
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


if __name__ == "__main__":
    main()
