# Releasing

## Branches and remotes

| Branch | Role | Rule |
|---|---|---|
| `develop` | day-to-day work | commit directly; longer work may use `feat/*`, deleted after merging |
| `main` | stable | only `git merge --ff-only develop` |
| `github` | public snapshot | a delayed copy of `main`, moved only by `scripts/sync-github.sh` |

`origin` is the canonical remote. GitHub (`UMa-Technology/unisolver`, remote `github`) receives
`main` and the `v*` tags from `scripts/sync-github.sh` and nothing else. Nothing is published to
crates.io or pub.dev: depend on the repository by git URL and tag.

```bash
git remote add github git@github.com:UMa-Technology/unisolver.git   # once per clone
```

## Cutting a release

```bash
cargo xtask release prepare 0.2.1      # on a clean develop
git checkout main && git merge --ff-only develop
scripts/release.sh v0.2.1              # checks, local gate, tag, atomic push of main + tag
scripts/sync-github.sh                 # GitHub main and tags; the tag starts the release workflow
git checkout develop
```

- `cargo xtask release prepare X.Y.Z` writes the version into `Cargo.toml` (`[workspace.package]`),
  the plugin's `pubspec.yaml`, both podspecs and the example app's `pubspec.lock`; turns
  `## Unreleased` into `## YYYY-MM-DD — vX.Y.Z` under a new, empty `## Unreleased`; refreshes
  `Cargo.lock`; and commits `chore(release): vX.Y.Z`. It refuses an empty Unreleased section, a
  version that already has a heading, a dirty tree and any branch but `develop`. When the version
  changes, the example app's `Podfile.lock` files still name the old one: rebuild the example for
  macOS and iOS (`flutter build macos`, `flutter build ios --config-only --no-codesign`) and
  commit them before merging into `main`.
- `scripts/release.sh vX.Y.Z [--no-push]` refuses to run off `main`, on a dirty tree, for an
  existing tag, or while `origin/main` has commits `main` lacks. It then runs
  `cargo xtask release check vX.Y.Z` (every manifest carries the version, the newest CHANGELOG
  heading is the tag, nothing waits under Unreleased) and the full gate `scripts/ci/ci-local.sh`,
  creates the annotated tag and pushes `main` and the tag in one atomic push.
- `cargo xtask release notes vX.Y.Z` prints the release's CHANGELOG section: the GitHub release
  notes.

User-visible changes get their CHANGELOG entry under `## Unreleased` in the commit that makes them;
that is where the reasons and measurements behind a one-line commit message go.

## What a GitHub release contains

| Asset | Contents |
|---|---|
| `unisolver-cabi-vX.Y.Z-<platform>.zip` | `include/unisolver.h`, the dynamic and static libraries under `lib/`, `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD_PARTY_LICENSES.md`; platforms `macos-universal`, `linux-x86_64`, `windows-x86_64`, `windows-aarch64` |
| `unisolver-cabi-vX.Y.Z-ios.zip` | `unisolver.xcframework` (the static library for devices, arm64, and simulators, arm64 + x86_64; iOS 12 or later; headers with a module map), `include/unisolver.h` and the license files |
| `unisolver-cabi-vX.Y.Z-android.zip` | `lib/arm64-v8a/` and `lib/x86_64/libunisolver_cabi.so` (API 21 or later, 16 KB page aligned), `include/unisolver.h` and the license files |
| `unisolver_names.bin`, `unisolver_names.NOTICE.txt` | the optional names pack (GPL-2.0-or-later) and its notice |
| `SHA256SUMS` | checksums of every asset above |

No star databases are attached; the wide-field tier ships inside the Flutter plugin. Flutter apps
use the plugin, which builds the library itself; the iOS and Android packages are for native apps.
`scripts/ci/package_cabi.py` builds each package and links a small program against it.
`gh workflow run release.yml` is a dry run: it builds the same zips as workflow artifacts and
publishes nothing.

## CI

`scripts/ci/ci-local.sh` is the gate. GitHub Actions runs `.github/workflows/ci.yml` when `main`
reaches GitHub. While the repository is private only the Linux job runs;
`gh workflow run ci.yml -f full=true` adds the macOS, Windows and Flutter jobs. Once public, every
job runs on every push and pull request.
