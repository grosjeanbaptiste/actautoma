//! `actautoma commit --path P... -m MESSAGE [--trailer TEXT] [--dir DIR]`:
//! commits exactly the listed paths, never a peer's file, never `--no-verify`.

use crate::forge::{Error, Runner};
use std::io::Write;
use std::path::Path;

pub const USAGE: &str =
    "usage: actautoma commit --path PATH... -m MESSAGE [--trailer TEXT] [--dir DIR]";

const GIT: &str = "git";
const FLAG_PATH: &str = "--path";
const FLAG_MESSAGE: &str = "-m";
const FLAG_TRAILER: &str = "--trailer";
const FLAG_DIR: &str = "--dir";
const PATH_SEPARATOR: &str = "--";
const TRAILER_SEPARATOR: &str = "\n\n";

struct Request {
    paths: Vec<String>,
    message: String,
    dir: std::path::PathBuf,
}

fn usage() -> Error {
    Error::Run(USAGE.to_string())
}

fn parse(args: &[String], cwd: &Path) -> Result<Request, Error> {
    let mut paths = Vec::new();
    let mut message = String::new();
    let mut trailer = None;
    let mut dir = cwd.to_path_buf();
    let mut rest = args.iter().skip(1);
    while let Some(option) = rest.next() {
        let value = rest.next().ok_or_else(usage)?;
        match option.as_str() {
            FLAG_PATH => paths.push(value.clone()),
            FLAG_MESSAGE => message.clone_from(value),
            FLAG_TRAILER => trailer = Some(value.clone()),
            FLAG_DIR => dir = cwd.join(value),
            _ => return Err(usage()),
        }
    }
    if paths.is_empty() || message.trim().is_empty() {
        return Err(usage());
    }
    if let Some(trailer) = trailer {
        message = format!("{message}{TRAILER_SEPARATOR}{trailer}");
    }
    Ok(Request {
        paths,
        message,
        dir,
    })
}

fn git(runner: &dyn Runner, dir: &Path, head: &[&str], paths: &[String]) -> Result<String, Error> {
    let args: Vec<&str> = head
        .iter()
        .copied()
        .chain(paths.iter().map(String::as_str))
        .collect();
    runner.run(GIT, &args, Some(dir))
}

pub fn run(
    args: &[String],
    cwd: &Path,
    runner: &dyn Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let request = parse(args, cwd)?;
    let (dir, paths) = (request.dir.as_path(), request.paths.as_slice());
    let changed = git(
        runner,
        dir,
        &["status", "--porcelain", PATH_SEPARATOR],
        paths,
    )?;
    if changed.trim().is_empty() {
        return Err(Error::Run(format!(
            "nothing to commit in {}",
            paths.join(", ")
        )));
    }
    git(runner, dir, &["add", PATH_SEPARATOR], paths)?;
    git(
        runner,
        dir,
        &["commit", FLAG_MESSAGE, &request.message, PATH_SEPARATOR],
        paths,
    )?;
    let sha = git(runner, dir, &["rev-parse", "--short", "HEAD"], &[])?;
    writeln!(out, "committed {}", sha.trim())
        .map_err(|e| Error::Run(format!("cannot write the answer: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fake;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("commit")
            .chain(list.iter().copied())
            .map(String::from)
            .collect()
    }

    fn go(list: &[&str], fake: &Fake) -> (Result<(), Error>, String) {
        let mut out = Vec::new();
        let r = run(&args(list), Path::new("/repo"), fake, &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    fn refused(list: &[&str], needle: &str) {
        let fake = Fake::answering(vec![]);
        let (r, _) = go(list, &fake);
        let msg = format!("{:?}", r.unwrap_err());
        assert!(msg.contains(needle), "{msg}");
        assert_eq!(fake.calls_len(), 0, "nothing may run");
    }

    #[test]
    fn needs_a_path_and_a_message() {
        refused(&["-m", "x"], "usage: actautoma commit");
        refused(&["--path", "a"], "usage: actautoma commit");
        refused(&["--path", "a", "-m", "  "], "usage: actautoma commit");
    }

    #[test]
    fn never_accepts_no_verify_or_an_unknown_option() {
        refused(
            &["--path", "a", "-m", "x", "--no-verify"],
            "usage: actautoma commit",
        );
        refused(
            &["--path", "a", "-m", "x", "--amend"],
            "usage: actautoma commit",
        );
    }

    #[test]
    fn a_path_with_no_change_is_refused_naming_it_and_nothing_is_committed() {
        let fake = Fake::answering(vec![Ok(String::new())]);
        let (r, _) = go(&["--path", "a.txt", "-m", "x"], &fake);
        assert!(format!("{:?}", r.unwrap_err()).contains("nothing to commit in a.txt"));
        assert_eq!(fake.calls_len(), 1, "only the status check ran");
    }

    #[test]
    fn stages_then_commits_only_the_listed_paths_and_reports_the_sha() {
        let fake = Fake::answering(vec![
            Ok(" M a.txt\n".into()),
            Ok(String::new()),
            Ok(String::new()),
            Ok("abc1234\n".into()),
        ]);
        let (r, out) = go(
            &["--path", "a.txt", "--path", "b.txt", "-m", "fix: x"],
            &fake,
        );
        r.unwrap();
        assert_eq!(out.trim(), "committed abc1234");
        let (prog, status, _) = fake.call(0);
        assert_eq!(prog, "git");
        assert_eq!(status, ["status", "--porcelain", "--", "a.txt", "b.txt"]);
        assert_eq!(fake.call(1).1, ["add", "--", "a.txt", "b.txt"]);
        assert_eq!(
            fake.call(2).1,
            ["commit", "-m", "fix: x", "--", "a.txt", "b.txt"]
        );
        assert_eq!(fake.call(3).1, ["rev-parse", "--short", "HEAD"]);
    }

    #[test]
    fn the_trailer_is_appended_after_a_blank_line() {
        let fake = Fake::answering(vec![
            Ok("M a\n".into()),
            Ok(String::new()),
            Ok(String::new()),
            Ok("abc\n".into()),
        ]);
        go(
            &[
                "--path",
                "a",
                "-m",
                "msg",
                "--trailer",
                "Co-Authored-By: T <t@t>",
            ],
            &fake,
        )
        .0
        .unwrap();
        assert_eq!(fake.call(2).1[2], "msg\n\nCo-Authored-By: T <t@t>");
    }

    #[test]
    fn runs_in_the_dir_when_given() {
        let fake = Fake::answering(vec![
            Ok("M a\n".into()),
            Ok(String::new()),
            Ok(String::new()),
            Ok("abc\n".into()),
        ]);
        go(&["--path", "a", "-m", "m", "--dir", "sub"], &fake)
            .0
            .unwrap();
        assert_eq!(fake.call(0).2, Some(Path::new("/repo/sub").to_path_buf()));
    }

    #[test]
    fn a_failing_commit_is_an_error_and_prints_nothing() {
        let fake = Fake::answering(vec![
            Ok("M a\n".into()),
            Ok(String::new()),
            Err(Error::Run("hook failed".into())),
        ]);
        let (r, out) = go(&["--path", "a", "-m", "m"], &fake);
        assert!(format!("{:?}", r.unwrap_err()).contains("hook failed"));
        assert!(out.is_empty());
    }
}
