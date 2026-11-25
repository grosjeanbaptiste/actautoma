//! `actautoma catch-up [--base BRANCH] [--dir DIR]`: the verb over
//! `branches::catch_up`. Conflict and uncommitted changes are errors, so the
//! exit status says the branch was not caught up.

use crate::branches::{catch_up_with, CatchUp, Options};
use crate::forge::{Error, Runner};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const USAGE: &str =
    "usage: actautoma catch-up [--base BRANCH] [--dir DIR] [--union] [--gate COMMAND]";

pub fn run(
    args: &[String],
    cwd: &Path,
    runner: &dyn Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let mut base = "main".to_string();
    let mut dir: PathBuf = cwd.to_path_buf();
    let mut options = Options::default();
    let mut rest = args.iter().skip(1);
    while let Some(option) = rest.next() {
        if option == "--union" {
            options.union = true;
            continue;
        }
        let value = rest.next().ok_or_else(|| Error::Run(USAGE.to_string()))?;
        match option.as_str() {
            "--base" => base.clone_from(value),
            "--dir" => dir = cwd.join(value),
            "--gate" => options.gate = Some(value.clone()),
            _ => return Err(Error::Run(USAGE.to_string())),
        }
    }
    let line = match catch_up_with(runner, &dir, &base, &options)? {
        CatchUp::UpToDate { ahead } => format!("up to date, {ahead} commit(s) to push"),
        CatchUp::FastForwarded { commits } => format!("fast-forwarded {commits} commit(s)"),
        CatchUp::Replayed { commits, backup } => {
            format!("replayed {commits} commit(s) on origin/{base}; original kept as {backup}")
        }
        CatchUp::Conflict { files, backup } => {
            return Err(Error::Run(format!(
                "conflict in {}; branch untouched, original kept as {backup}",
                files.join(", ")
            )))
        }
        CatchUp::Resolved {
            commits,
            backup,
            files,
        } => format!(
            "replayed {commits} commit(s) on origin/{base}, kept both sides in {}; original kept as {backup}",
            files.join(", ")
        ),
        CatchUp::GateFailed { backup, output } => {
            return Err(Error::Run(format!(
                "gate failed, branch put back as it was; original kept as {backup}: {output}"
            )))
        }
        CatchUp::Dirty { files } => {
            return Err(Error::Run(format!(
                "uncommitted changes in {}",
                files.join(", ")
            )))
        }
    };
    writeln!(out, "{line}").map_err(|e| Error::Run(format!("cannot write the answer: {e}")))
}
