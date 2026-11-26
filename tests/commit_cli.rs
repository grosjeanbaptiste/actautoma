//! `actautoma commit` on a real repository: only the listed paths are committed.

use actautoma::cli::dispatch;
use actautoma::forge::Error;
use actautoma::gh::Gh;
use actautoma::process::ProcessRunner;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository holding `mine.txt` and `peer.txt`, both committed.
fn repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "actautoma-commit-cli-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    for f in ["mine.txt", "peer.txt"] {
        std::fs::write(dir.join(f), "v1\n").unwrap();
    }
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    dir
}

fn commit(cwd: &Path, args: &[&str]) -> (Result<(), Error>, String) {
    let mut all = vec!["commit".to_string()];
    all.extend(args.iter().map(ToString::to_string));
    let env = |_: &str| None;
    let mut out = Vec::new();
    let result = dispatch(
        &all,
        cwd,
        &env,
        &Gh::new(ProcessRunner),
        &ProcessRunner,
        &mut out,
    );
    (result, String::from_utf8(out).unwrap())
}

#[test]
fn a_staged_file_of_a_peer_is_not_committed() {
    let dir = repo();
    std::fs::write(dir.join("mine.txt"), "v2\n").unwrap();
    std::fs::write(dir.join("peer.txt"), "peer\n").unwrap();
    git(&dir, &["add", "peer.txt"]);
    let (result, out) = commit(&dir, &["--path", "mine.txt", "-m", "mine only"]);
    result.unwrap();
    assert!(out.starts_with("committed "), "{out}");
    assert_eq!(
        git(&dir, &["show", "--name-only", "--format=", "HEAD"]),
        "mine.txt"
    );
    assert_eq!(git(&dir, &["status", "--porcelain"]), "M  peer.txt");
}

#[test]
fn the_trailer_ends_the_message() {
    let dir = repo();
    std::fs::write(dir.join("mine.txt"), "v2\n").unwrap();
    commit(
        &dir,
        &[
            "--path",
            "mine.txt",
            "-m",
            "subject",
            "--trailer",
            "Co-Authored-By: T <t@t>",
        ],
    )
    .0
    .unwrap();
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%B"]),
        "subject\n\nCo-Authored-By: T <t@t>"
    );
}

#[test]
fn an_unchanged_path_is_refused_and_no_commit_is_made() {
    let dir = repo();
    let before = git(&dir, &["rev-parse", "HEAD"]);
    let err = commit(&dir, &["--path", "mine.txt", "-m", "x"])
        .0
        .unwrap_err();
    assert!(format!("{err:?}").contains("nothing to commit in mine.txt"));
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]), before);
}
