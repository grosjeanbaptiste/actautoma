//! `actautoma catch-up`: the verb over `branches::catch_up`, on real repositories.

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

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

/// `(work, peer)`: two clones of one bare origin holding `a.txt`.
fn pair() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "actautoma-catchup-cli-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    let origin = root.join("origin.git");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    // the replay commits, so the repository carries its own identity
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@t"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    commit(&work, "a.txt", "one\ntwo\nthree\n");
    git(&work, &["push", "-q", "origin", "main"]);
    git(&root, &["clone", "-q", origin.to_str().unwrap(), "peer"]);
    (work, root.join("peer"))
}

fn catch_up(cwd: &Path, args: &[&str]) -> (Result<(), Error>, String) {
    let mut all = vec!["catch-up".to_string()];
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

fn peer_ships(peer: &Path, file: &str, text: &str) {
    commit(peer, file, text);
    git(peer, &["push", "-q", "origin", "main"]);
}

#[test]
fn a_diverged_branch_is_replayed_and_the_copy_is_named() {
    let (work, peer) = pair();
    commit(&work, "mine.txt", "m\n");
    let tip = git(&work, &["rev-parse", "HEAD"]);
    peer_ships(&peer, "p.txt", "p\n");
    let (result, out) = catch_up(&work, &[]);
    result.unwrap();
    assert_eq!(
        out,
        format!("replayed 1 commit(s) on origin/main; original kept as backup/main/{tip}\n")
    );
}

#[test]
fn up_to_date_and_fast_forward_are_reported() {
    let (work, peer) = pair();
    commit(&work, "mine.txt", "m\n");
    assert_eq!(catch_up(&work, &[]).1, "up to date, 1 commit(s) to push\n");
    let (work, peer2) = pair();
    let _ = peer;
    peer_ships(&peer2, "p.txt", "p\n");
    assert_eq!(catch_up(&work, &[]).1, "fast-forwarded 1 commit(s)\n");
}

#[test]
fn a_conflict_is_an_error_naming_files_and_copy() {
    let (work, peer) = pair();
    commit(&work, "a.txt", "one\nMINE\nthree\n");
    let tip = git(&work, &["rev-parse", "HEAD"]);
    peer_ships(&peer, "a.txt", "one\nPEER\nthree\n");
    let (result, out) = catch_up(&work, &[]);
    match result {
        Err(Error::Run(m)) => assert_eq!(
            m,
            format!("conflict in a.txt; branch untouched, original kept as backup/main/{tip}")
        ),
        other => panic!("{other:?}"),
    }
    assert!(out.is_empty());
}

#[test]
fn a_dirty_tree_is_an_error_naming_files() {
    let (work, _peer) = pair();
    std::fs::write(work.join("a.txt"), "edited\n").unwrap();
    match catch_up(&work, &[]).0 {
        Err(Error::Run(m)) => assert_eq!(m, "uncommitted changes in a.txt"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn dir_and_base_options_are_read_and_unknown_ones_refused() {
    let (work, peer) = pair();
    peer_ships(&peer, "p.txt", "p\n");
    let elsewhere = std::env::temp_dir();
    let (result, out) = catch_up(
        &elsewhere,
        &["--dir", work.to_str().unwrap(), "--base", "main"],
    );
    result.unwrap();
    assert_eq!(out, "fast-forwarded 1 commit(s)\n");
    match catch_up(&work, &["--zap"]).0 {
        Err(Error::Run(m)) => assert!(m.contains("usage: actautoma catch-up"), "{m}"),
        other => panic!("{other:?}"),
    }
}
