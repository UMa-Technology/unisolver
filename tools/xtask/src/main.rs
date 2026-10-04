//! `cargo xtask <command>`: repository maintenance. See docs/upstream.md and docs/releasing.md.
use clap::{Parser, Subcommand};
use std::path::Path;
use std::process::{Command, ExitCode};
use xtask::release;
use xtask::upstream::changelog::Flag;
use xtask::upstream::git::numstat;
use xtask::upstream::tree::Change;
use xtask::upstream::{self, Ctx, SyncOutcome};

#[derive(Parser)]
#[command(
    name = "cargo xtask",
    bin_name = "cargo xtask",
    about = "Repository maintenance commands"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Vendored upstreams and their patch queues (docs/upstream.md)
    Upstream {
        /// Upstream to act on (tetra3, seiza); `check` without it checks every upstream
        #[arg(long)]
        name: Option<String>,
        #[command(subcommand)]
        cmd: UpstreamCmd,
    },
    /// Release bookkeeping (docs/releasing.md)
    Release {
        #[command(subcommand)]
        cmd: ReleaseCmd,
    },
}

#[derive(Subcommand)]
enum ReleaseCmd {
    /// On a clean develop: set every manifest to X.Y.Z, date the CHANGELOG's Unreleased entries
    /// as vX.Y.Z, refresh Cargo.lock and commit
    Prepare {
        /// Version, e.g. 0.2.1
        version: String,
        /// Release date, YYYY-MM-DD (default: today)
        #[arg(long)]
        date: Option<String>,
    },
    /// Exit 1 unless every manifest and the CHANGELOG are ready for tag vX.Y.Z
    Check { tag: String },
    /// Print the CHANGELOG section of vX.Y.Z (the GitHub release notes)
    Notes { tag: String },
}

#[derive(Subcommand)]
enum UpstreamCmd {
    /// Verify third_party/<name> = locked tag + patches, then look for newer upstream releases.
    /// Exit 0 in sync, 1 local drift, 2 behind upstream, 3 could not verify (the worst of all
    /// upstreams when `--name` is not given)
    Check,
    /// Rebuild branch `unisolver` in target/upstream/work: the locked tag plus one commit per patch
    Edit {
        /// Discard unexported commits and uncommitted changes in the work clone
        #[arg(long)]
        force: bool,
    },
    /// Write branch `unisolver` back to third_party/<name> and the patch files
    Export,
    /// Rebase the queue onto another upstream tag, export it and update the lock
    Sync {
        /// Upstream tag, e.g. v0.14.0
        #[arg(required_unless_present_any = ["cont", "abort"])]
        tag: Option<String>,
        /// Finish a sync after resolving its rebase conflict
        #[arg(long = "continue", conflicts_with_all = ["tag", "abort"])]
        cont: bool,
        /// Give up an interrupted sync
        #[arg(long, conflicts_with = "tag")]
        abort: bool,
        /// Skip the test runs after a successful sync
        #[arg(long)]
        no_test: bool,
    },
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("tools/xtask sits two levels below the repository root");
    match Cli::parse().cmd {
        Cmd::Upstream { name, cmd } => upstream_main(root, name, cmd),
        Cmd::Release { cmd } => release_main(root, cmd),
    }
}

fn upstream_main(root: &Path, name: Option<String>, cmd: UpstreamCmd) -> ExitCode {
    let specs: Vec<&'static upstream::Spec> = match &name {
        Some(n) => match upstream::spec(n) {
            Ok(s) => vec![s],
            Err(e) => return fail(e),
        },
        None => upstream::UPSTREAMS.iter().collect(),
    };
    if !matches!(cmd, UpstreamCmd::Check) && specs.len() != 1 {
        return fail(anyhow::anyhow!(
            "name the upstream: `cargo xtask upstream --name <tetra3|seiza> ...`"
        ));
    }
    let load = |s: &'static upstream::Spec| Ctx::load(root, s, std::env::var(s.repo_env).ok());
    if matches!(cmd, UpstreamCmd::Check) {
        let mut code = 0;
        for (i, s) in specs.into_iter().enumerate() {
            if i > 0 {
                println!();
            }
            match load(s) {
                Ok(ctx) => code = code.max(check(&ctx)),
                Err(e) => {
                    eprintln!("error: {}: {e:#}", s.name);
                    code = code.max(3);
                }
            }
        }
        return ExitCode::from(code);
    }
    let ctx = match load(specs[0]) {
        Ok(ctx) => ctx,
        Err(e) => return fail(e),
    };
    let n = ctx.spec.name;
    let result = match cmd {
        UpstreamCmd::Check => unreachable!("handled above"),
        UpstreamCmd::Edit { force } => upstream::edit(&ctx, force).map(|work| {
            println!("branch {} in {}: the locked tag + one commit per patch", upstream::BRANCH, work.display());
            println!("amend or add commits there (a commit's subject becomes its patch file name),");
            println!("then run `cargo xtask upstream --name {n} export`");
        }),
        UpstreamCmd::Export => upstream::export(&ctx).map(|count| {
            println!("exported {count} patch(es) and third_party/{n}; review with `git status` and `git diff`");
        }),
        UpstreamCmd::Sync { abort: true, .. } => upstream::sync_abort(&ctx).map(|()| {
            println!("sync abandoned: the lock, the queue and third_party/{n} are unchanged");
        }),
        UpstreamCmd::Sync { tag, cont, no_test, .. } => {
            let outcome = if cont {
                upstream::sync_continue(&ctx)
            } else {
                upstream::sync(&ctx, tag.as_deref().expect("clap requires a tag"))
            };
            match outcome {
                Ok(outcome) => return ExitCode::from(report_sync(&ctx, outcome, no_test)),
                Err(e) => Err(e),
            }
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn release_main(root: &Path, cmd: ReleaseCmd) -> ExitCode {
    let result = match cmd {
        ReleaseCmd::Prepare { version, date } => prepare(root, &version, date),
        ReleaseCmd::Check { tag } => match release::check(root, &tag) {
            Ok(problems) if problems.is_empty() => {
                println!("{tag}: every manifest and the CHANGELOG are ready");
                Ok(())
            }
            Ok(problems) => {
                for p in &problems {
                    println!("NOT READY: {p}");
                }
                return ExitCode::from(1);
            }
            Err(e) => Err(e),
        },
        ReleaseCmd::Notes { tag } => release::notes(root, &tag).map(|n| print!("{n}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

/// `release prepare`: on a clean develop, rewrite the manifests and the CHANGELOG, refresh
/// Cargo.lock and commit.
fn prepare(root: &Path, version: &str, date: Option<String>) -> anyhow::Result<()> {
    let branch = git(root, &["branch", "--show-current"])?;
    anyhow::ensure!(
        branch == "develop",
        "prepare releases on develop (on {branch})"
    );
    let dirty = git(root, &["status", "--porcelain"])?;
    anyhow::ensure!(dirty.is_empty(), "the working tree is not clean");
    let tag = format!("v{version}");
    anyhow::ensure!(
        git(root, &["tag", "-l", &tag])?.is_empty(),
        "tag {tag} exists"
    );
    let date = match date {
        Some(d) => d,
        None => today()?,
    };
    release::prepare(root, version, &date)?;
    anyhow::ensure!(
        cargo(root, &["update", "--workspace", "--quiet"]),
        "cargo update --workspace failed"
    );
    let mut add = vec!["add", "--"];
    add.extend(release::prepared_files());
    git(root, &add)?;
    git(
        root,
        &["commit", "--quiet", "-m", &format!("chore(release): {tag}")],
    )?;
    println!("committed chore(release): {tag}");
    println!("next: git checkout main && git merge --ff-only develop && scripts/release.sh {tag}");
    Ok(())
}

/// git as the maintainer: their identity and hooks (the upstream clones use `Git` instead).
fn git(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn today() -> anyhow::Result<String> {
    let out = Command::new("date").arg("+%F").output()?;
    anyhow::ensure!(out.status.success(), "date +%F failed");
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn fail(e: anyhow::Error) -> ExitCode {
    eprintln!("error: {e:#}");
    ExitCode::from(1)
}

fn check(ctx: &Ctx) -> u8 {
    let patches = upstream::read_series(ctx).map(|s| s.len()).unwrap_or(0);
    let n = ctx.spec.name;
    println!(
        "== {n} local: third_party/{n} = {} + {patches} patch(es)",
        ctx.lock.tag
    );
    let mut code = 0;
    match upstream::check_local(ctx) {
        Ok(r) if r.is_clean() => println!("OK: byte-identical"),
        Ok(r) => {
            code = 1;
            for p in &r.problems {
                println!("QUEUE: {p}");
            }
            for c in &r.changes {
                print_change(ctx, c);
            }
            println!("third_party/{n} is generated: change it with `cargo xtask upstream --name {n} edit`, then `export` (docs/upstream.md)");
        }
        Err(e) => {
            println!("ERROR: cannot verify: {e:#}");
            return 3;
        }
    }
    println!();
    println!("== {n} upstream: {}", ctx.repo_url);
    match upstream::check_remote(ctx) {
        Err(e) => println!("SKIP: {e:#} (the local check above still holds)"),
        Ok(r) => {
            if let Some(m) = &r.tag_moved {
                println!("WARNING: tag {} {m}", ctx.lock.tag);
            }
            match r.newer.last() {
                None => println!("OK: {} is the latest release", ctx.lock.tag),
                Some(latest) => {
                    println!(
                        "BEHIND: {} newer release(s), {} -> {latest}",
                        r.newer.len(),
                        ctx.lock.tag
                    );
                    print_entries(&r.entries);
                }
            }
            if let Some(note) = &r.note {
                println!("{note}");
            }
            if r.is_behind() && code == 0 {
                code = 2;
            }
        }
    }
    code
}

fn print_change(ctx: &Ctx, change: &Change) {
    match change {
        Change::Modified(p) => {
            let stat = match numstat(&ctx.expected_dir().join(p), &ctx.vendored().join(p)) {
                Ok(Some((add, del))) => format!("+{add} -{del}"),
                _ => "binary".to_string(),
            };
            println!("DRIFT: M {p} ({stat})");
        }
        Change::Extra(p) => println!("DRIFT: A {p} (not produced by the queue)"),
        Change::Missing(p) => println!("DRIFT: D {p} (missing)"),
    }
}

fn print_entries(entries: &[(Flag, String)]) {
    for (flag, line) in entries {
        let short: String = line.chars().take(150).collect();
        println!("{} {short}", flag.marker());
    }
    if !entries.is_empty() {
        println!("!! = touches the patch surface, or asks for regenerated databases");
        println!(" ! = new or changed config field, or a breaking change");
    }
}

fn report_sync(ctx: &Ctx, outcome: SyncOutcome, no_test: bool) -> u8 {
    match outcome {
        SyncOutcome::Noop => {
            println!("already at {}: nothing to do", ctx.lock.tag);
            0
        }
        SyncOutcome::Conflict { work } => {
            println!("the rebase stopped on a conflict in {}", work.display());
            println!(
                "  resolve the files, `git add` them, `git -C {} rebase --continue`,",
                work.display()
            );
            println!(
                "  then `cargo xtask upstream sync --continue` (or `sync --abort` to give up)"
            );
            1
        }
        SyncOutcome::Synced { from, to, entries } => {
            let n = ctx.spec.name;
            println!("synced {from} -> {to}: third_party/{n}, the patches and the lock are updated (not committed)");
            if !no_test {
                let mut runs: Vec<Vec<&str>> = ctx
                    .spec
                    .test_packages
                    .iter()
                    .map(|p| vec!["test", "-p", p, "--release"])
                    .collect();
                runs.push(vec![
                    "test",
                    "--workspace",
                    "--release",
                    "--features",
                    "imageio satellites",
                ]);
                for args in runs {
                    if !cargo(&ctx.root, &args) {
                        println!("FAILED: cargo {}", args.join(" "));
                        return 1;
                    }
                }
            }
            println!();
            println!("upstream changelog {from} -> {to}:");
            print_entries(&entries);
            println!();
            println!("before committing:");
            println!("  1. review `git diff` of third_party/{n}, third_party/{n}-patches and third_party/{n}.lock");
            println!("  2. for !! entries: {}", ctx.spec.review_hint);
            println!("  3. add the sync to \"Sync history\" in third_party/{n}-patches/README.md, and a CHANGELOG entry");
            println!("  4. commit as `chore({n}): sync to {to}`");
            0
        }
    }
}

fn cargo(root: &Path, args: &[&str]) -> bool {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    println!("==> cargo {}", args.join(" "));
    Command::new(cargo)
        .args(args)
        .current_dir(root)
        .status()
        .is_ok_and(|s| s.success())
}
