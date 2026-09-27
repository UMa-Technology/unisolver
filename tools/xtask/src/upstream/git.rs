//! A thin wrapper over the `git` CLI. Every call pins the settings that would otherwise make
//! results depend on the maintainer's configuration: line endings, signing, committer identity.
use anyhow::{bail, ensure, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Committer of every commit the commands create, and the author written into exported
/// patches. Not a real mailbox, so no maintainer address is published.
pub const IDENTITY_NAME: &str = "unisolver";
pub const IDENTITY_EMAIL: &str = "patches@unisolver.invalid";

pub struct Git {
    dir: PathBuf,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Git {
        Git { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `git -C <dir>` with the pinned settings; add a subcommand and arguments.
    pub fn command(&self) -> Command {
        let mut c = Command::new("git");
        c.arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.autocrlf=false"])
            .args(["-c", "commit.gpgsign=false"])
            .env("GIT_COMMITTER_NAME", IDENTITY_NAME)
            .env("GIT_COMMITTER_EMAIL", IDENTITY_EMAIL)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        c
    }

    /// Runs `git <args>` and returns its stdout; a non-zero exit is an error carrying stderr.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let out = self
            .command()
            .args(args)
            .output()
            .with_context(|| format!("running git {}", args.join(" ")))?;
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Whether `git <args>` exits 0 (for commands whose status is the answer).
    pub fn succeeds(&self, args: &[&str]) -> bool {
        self.command()
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// `run` for network commands: killed after `timeout`, since GitHub is not always reachable.
    pub fn run_timeout(&self, args: &[&str], timeout: Duration) -> Result<String> {
        let mut child = self
            .command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running git {}", args.join(" ")))?;
        let out = drain(child.stdout.take().expect("piped"));
        let err = drain(child.stderr.take().expect("piped"));
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "git {} timed out after {} s",
                    args.join(" "),
                    timeout.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let out = out.join().expect("reader thread");
        let err = err.join().expect("reader thread");
        if !status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&err).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

/// `(added, removed)` line counts between two files; `None` when git sees binary content.
pub fn numstat(a: &Path, b: &Path) -> Result<Option<(u64, u64)>> {
    let out = Command::new("git")
        .args(["diff", "--no-index", "--no-ext-diff", "--numstat", "--"])
        .arg(a)
        .arg(b)
        .output()
        .context("running git diff --no-index")?;
    // --no-index exits 1 when the files differ
    ensure!(
        matches!(out.status.code(), Some(0 | 1)),
        "git diff --no-index failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let mut fields = text.split('\t');
    let mut count = || fields.next().and_then(|s| s.trim().parse::<u64>().ok());
    Ok(count().zip(count()))
}

/// A path as the `&str` git arguments need.
pub fn path_str(p: &Path) -> Result<&str> {
    p.to_str()
        .with_context(|| format!("non-UTF-8 path {}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_counts_changed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, "one\ntwo\n").unwrap();
        std::fs::write(&b, "one\n2\nthree\n").unwrap();
        assert_eq!(numstat(&a, &b).unwrap(), Some((2, 1)));
    }
}
