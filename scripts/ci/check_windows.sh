#!/usr/bin/env bash
# Windows cross-check from macOS:
#   - unisolver-core has no C dependencies, so both Windows targets get a full cargo check
#   - unisolver_frb depends on flutter_rust_bridge → dart-sys (C, needs MSVC headers) and
#     cannot be cross-checked; verify it on a Windows host:
#       cd packages/unisolver_flutter/example && flutter build windows
set -e
cargo check -p unisolver-core --target x86_64-pc-windows-msvc
cargo check -p unisolver-core --target aarch64-pc-windows-msvc
echo "windows check OK: unisolver-core on both arches (frb layer needs a Windows host, see comments)"
