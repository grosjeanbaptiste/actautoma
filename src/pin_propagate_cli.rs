//! Command line of `actautoma pin-propagate`: parse the arguments, call
//! [`crate::pin_propagate::propagate`], print one line per consumer.

use crate::forge::{Error, Runner};
use crate::gh::Gh;
use crate::pin_propagate::{propagate, Options, Outcome};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const USAGE: &str =
    "usage: actautoma pin-propagate DEP [--rev REV] [--root DIR] [--gate \"PROGRAM ARGS\"] [--dry-run] [--merge]";

fn usage() -> Error {
    Error::Run(USAGE.to_string())
}

/// One report line per consumer.
#[must_use]
pub fn line((name, outcome): &(String, Outcome)) -> String {
    match outcome {
        Outcome::UpToDate => format!("{name}: up to date"),
        Outcome::Planned => format!("{name}: planned"),
        Outcome::Opened { url } => format!("{name}: opened {url}"),
        Outcome::Merged { url } => format!("{name}: merged {url}"),
        Outcome::Held { url, reason } => format!("{name}: held {url}: {reason}"),
        Outcome::Blocked { on } => format!("{name}: blocked on {on}"),
        Outcome::Failed { step, output } => format!("{name}: failed at {step}: {output}"),
    }
}

/// `actautoma pin-propagate DEP [--rev REV] [--root DIR] [--gate CMD] [--dry-run]`. `REV` is the
/// tip of `origin/main` of `ROOT/DEP` (fetched first) when absent; `ROOT` is
/// `~/Source/Acteble/openacte` by default.
///
/// # Errors
/// On a bad command line, a plan that cannot be made, or a consumer that failed (after its line
/// is printed).
pub fn run(
    args: &[String],
    cwd: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    runner: &impl Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let mut it = args.iter().skip(1);
    let dep = it
        .next()
        .filter(|a| !a.starts_with("--"))
        .ok_or_else(usage)?;
    let (mut rev, mut root, mut gate, mut dry_run, mut merge) = (None, None, None, false, false);
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(usage);
        match flag.as_str() {
            "--dry-run" => dry_run = true,
            "--merge" => merge = true,
            "--rev" => rev = Some(value()?),
            "--root" => root = Some(cwd.join(value()?)),
            "--gate" => {
                let words: Vec<String> = value()?.split_whitespace().map(str::to_owned).collect();
                if words.is_empty() {
                    return Err(usage());
                }
                gate = Some(words);
            }
            _ => return Err(usage()),
        }
    }
    let root: PathBuf = root.unwrap_or_else(|| {
        PathBuf::from(env("HOME").unwrap_or_default()).join("Source/Acteble/openacte")
    });
    let rev = match rev {
        Some(rev) => rev,
        None => {
            let dir = root.join(dep);
            runner.run("git", &["fetch", "origin"], Some(&dir))?;
            runner
                .run("git", &["rev-parse", "origin/main"], Some(&dir))?
                .trim()
                .to_owned()
        }
    };
    let options = Options {
        gate,
        dry_run,
        merge,
        settle: Duration::from_secs(4),
    };
    let forge_for = |wt: &Path| Gh::new(runner).in_dir(wt);
    let results = propagate(runner, &forge_for, &root, dep, &rev, &options)?;
    for result in &results {
        writeln!(out, "{}", line(result)).map_err(|e| Error::Run(e.to_string()))?;
    }
    if results
        .iter()
        .any(|(_, o)| matches!(o, Outcome::Failed { .. }))
    {
        return Err(Error::Run(
            "pin-propagate stopped on a failed consumer".into(),
        ));
    }
    Ok(())
}
