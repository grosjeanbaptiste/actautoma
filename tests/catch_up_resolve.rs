//! `catch_up_with`: additive conflicts are settled by keeping both sides, the
//! replay is resumed by actautoma itself, and a failing gate puts the branch
//! back as it was.

use actautoma::branches::{catch_up_with, CatchUp, Options};
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

fn pair() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "actautoma-resolve-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    let origin = root.join("origin.git");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@t"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    commit(&work, "a.txt", "one\ntwo\nthree\n");
    git(&work, &["push", "-q", "origin", "main"]);
    git(&root, &["clone", "-q", origin.to_str().unwrap(), "peer"]);
    git(&root.join("peer"), &["config", "user.name", "t"]);
    git(&root.join("peer"), &["config", "user.email", "t@t"]);
    (work, root.join("peer"))
}

fn peer_ships(peer: &Path, file: &str, text: &str) {
    commit(peer, file, text);
    git(peer, &["push", "-q", "origin", "main"]);
}

fn union(gate: Option<&str>) -> Options {
    Options {
        union: true,
        gate: gate.map(String::from),
    }
}

/// Both sides add a line at the same place: git cannot choose.
fn additive_conflict() -> (PathBuf, String) {
    let (work, peer) = pair();
    commit(&work, "a.txt", "one\ntwo\nMINE\nthree\n");
    let original = git(&work, &["rev-parse", "HEAD"]);
    peer_ships(&peer, "a.txt", "one\ntwo\nPEER\nthree\n");
    (work, original)
}

#[test]
fn an_additive_conflict_keeps_both_sides_and_the_replay_finishes() {
    let (work, original) = additive_conflict();
    let outcome = catch_up_with(&ProcessRunner, &work, "main", &union(None)).unwrap();
    assert_eq!(
        outcome,
        CatchUp::Resolved {
            commits: 1,
            backup: format!("backup/main/{original}"),
            files: vec!["a.txt".to_string()],
        }
    );
    let text = std::fs::read_to_string(work.join("a.txt")).unwrap();
    assert!(text.contains("MINE") && text.contains("PEER"), "{text}");
    assert!(
        !text.contains("<<<<<<<") && !text.contains(">>>>>>>"),
        "{text}"
    );
    assert_eq!(git(&work, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&work, &["rev-parse", "HEAD~1"]),
        git(&work, &["rev-parse", "origin/main"])
    );
}

#[test]
fn without_the_union_option_a_conflict_still_stops() {
    let (work, original) = additive_conflict();
    let outcome = catch_up_with(&ProcessRunner, &work, "main", &Options::default()).unwrap();
    assert!(matches!(outcome, CatchUp::Conflict { .. }), "{outcome:?}");
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), original);
}

#[test]
fn a_failing_gate_puts_the_branch_back_as_it_was() {
    let (work, original) = additive_conflict();
    let outcome = catch_up_with(&ProcessRunner, &work, "main", &union(Some("exit 3"))).unwrap();
    match outcome {
        CatchUp::GateFailed { backup, .. } => assert_eq!(backup, format!("backup/main/{original}")),
        other => panic!("{other:?}"),
    }
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), original);
    assert_eq!(git(&work, &["status", "--porcelain"]), "");
}

#[test]
fn a_passing_gate_keeps_the_replay_and_runs_in_the_repository() {
    let (work, _) = additive_conflict();
    let outcome =
        catch_up_with(&ProcessRunner, &work, "main", &union(Some("test -f a.txt"))).unwrap();
    assert!(matches!(outcome, CatchUp::Resolved { .. }), "{outcome:?}");
}

#[test]
fn a_clean_replay_is_gated_too() {
    let (work, peer) = pair();
    commit(&work, "mine.txt", "m\n");
    let original = git(&work, &["rev-parse", "HEAD"]);
    peer_ships(&peer, "p.txt", "p\n");
    let outcome = catch_up_with(&ProcessRunner, &work, "main", &union(Some("exit 1"))).unwrap();
    assert!(matches!(outcome, CatchUp::GateFailed { .. }), "{outcome:?}");
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), original);
}

#[test]
fn a_conflict_that_is_not_additive_is_not_guessed() {
    let (work, peer) = pair();
    commit(&work, "a.txt", "one\nMINE\nthree\n");
    let original = git(&work, &["rev-parse", "HEAD"]);
    git(&peer, &["rm", "-q", "a.txt"]);
    git(&peer, &["commit", "-q", "-m", "delete"]);
    git(&peer, &["push", "-q", "origin", "main"]);
    let outcome = catch_up_with(&ProcessRunner, &work, "main", &union(None)).unwrap();
    assert!(matches!(outcome, CatchUp::Conflict { .. }), "{outcome:?}");
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), original);
    assert_eq!(git(&work, &["status", "--porcelain"]), "");
}

/// The whole merge is clean (the second commit undoes the first) yet replaying
/// the first commit alone conflicts: without `union` nothing is settled.
#[test]
fn a_conflict_found_only_during_the_replay_is_not_settled_without_union() {
    let (work, peer) = pair();
    commit(&work, "a.txt", "one\nMINE\nthree\n");
    commit(&work, "a.txt", "one\ntwo\nthree\n");
    let original = git(&work, &["rev-parse", "HEAD"]);
    peer_ships(&peer, "a.txt", "one\nPEER\nthree\n");
    let outcome = catch_up_with(&ProcessRunner, &work, "main", &Options::default());
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), original);
    assert_eq!(git(&work, &["status", "--porcelain"]), "");
}
