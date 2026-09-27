//! `cargo xtask <command>`: repository maintenance. See docs/upstream.md.
use clap::{Parser, Subcommand};
use std::path::Path;
use std::process::{Command, ExitCode};
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
    /// The vendored tetra3rs copy and its patch queue (docs/upstream.md)
    Upstream {
        #[command(subcommand)]
        cmd: UpstreamCmd,
    },
}

#[derive(Subcommand)]
enum UpstreamCmd {
    /// Verify third_party/tetra3 = locked tag + patches, then look for newer upstream releases.
    /// Exit 0 in sync, 1 local drift, 2 behind upstream, 3 could not verify
    Check,
    /// Rebuild branch `unisolver` in target/upstream/work: the locked tag plus one commit per patch
    Edit {
        /// Discard unexported commits and uncommitted changes in the work clone
        #[arg(long)]
        force: bool,
    },
    /// Write branch `unisolver` back to third_party/tetra3 and the patch files
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
    let Cmd::Upstream { cmd } = Cli::parse().cmd;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("tools/xtask sits two levels below the repository root");
    let ctx = match Ctx::load(root, std::env::var(upstream::REPO_ENV).ok()) {
        Ok(ctx) => ctx,
        Err(e) => return fail(e),
    };
    let result = match cmd {
        UpstreamCmd::Check => return ExitCode::from(check(&ctx)),
        UpstreamCmd::Edit { force } => upstream::edit(&ctx, force).map(|work| {
            println!("branch {} in {}: the locked tag + one commit per patch", upstream::BRANCH, work.display());
            println!("amend or add commits there (a commit's subject becomes its patch file name),");
            println!("then run `cargo xtask upstream export`");
        }),
        UpstreamCmd::Export => upstream::export(&ctx).map(|n| {
            println!("exported {n} patch(es) and third_party/tetra3; review with `git status` and `git diff`");
        }),
        UpstreamCmd::Sync { abort: true, .. } => upstream::sync_abort(&ctx).map(|()| {
            println!("sync abandoned: the lock, the queue and third_party/tetra3 are unchanged");
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

fn fail(e: anyhow::Error) -> ExitCode {
    eprintln!("error: {e:#}");
    ExitCode::from(1)
}

fn check(ctx: &Ctx) -> u8 {
    let patches = upstream::read_series(ctx).map(|s| s.len()).unwrap_or(0);
    println!(
        "== local: third_party/tetra3 = {} + {patches} patch(es)",
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
            println!("third_party/tetra3 is generated: change it with `cargo xtask upstream edit`, then `export` (docs/upstream.md)");
        }
        Err(e) => {
            println!("ERROR: cannot verify: {e:#}");
            return 3;
        }
    }
    println!();
    println!("== upstream: {}", ctx.repo_url);
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
            println!("synced {from} -> {to}: third_party/tetra3, the patches and the lock are updated (not committed)");
            if !no_test {
                let runs: [&[&str]; 2] = [
                    &["test", "-p", "tetra3", "--release"],
                    &[
                        "test",
                        "--workspace",
                        "--release",
                        "--features",
                        "imageio satellites",
                    ],
                ];
                for args in runs {
                    if !cargo(&ctx.root, args) {
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
            println!("  1. review `git diff` of third_party/tetra3, third_party/tetra3-patches and third_party/tetra3.lock");
            println!("  2. for !! entries: do the PatternEntry layout guards in storage.rs still hold, and must the database tiers be regenerated?");
            println!("  3. add the sync to \"Sync history\" in third_party/tetra3-patches/README.md, and a CHANGELOG entry");
            println!("  4. commit as `chore(tetra3): sync to {to}`");
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
