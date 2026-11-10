//! The real [`Runner`]: spawns the platform tool as a child process.

use crate::forge::{Error, Runner};
use std::path::Path;
use std::process::Command;

/// Runs the program with `std::process::Command`; stdout on success.
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, Error> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.current_dir(dir.unwrap_or_else(|| Path::new(".")));
        let out = cmd.output().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::Run(format!(
                "{program} not found (is it installed and on PATH?)"
            )),
            _ => Error::Run(format!("cannot run `{program}`: {e}")),
        })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(Error::Run(format!(
                "`{program}` failed ({}): {}",
                out.status,
                stderr.trim()
            )));
        }
        String::from_utf8(out.stdout)
            .map_err(|_| Error::Parse(format!("`{program}` printed non-UTF-8 output")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stdout() {
        assert_eq!(ProcessRunner.run("echo", &["hi"], None).unwrap(), "hi\n");
    }

    #[test]
    fn runs_in_the_given_directory() {
        let out = ProcessRunner.run("pwd", &[], Some(Path::new("/"))).unwrap();
        assert_eq!(out, "/\n");
    }

    #[test]
    fn non_zero_exit_is_an_error_carrying_stderr() {
        let err = ProcessRunner
            .run("sh", &["-c", "echo boom >&2; exit 3"], None)
            .unwrap_err();
        assert!(
            matches!(&err, Error::Run(m) if m.contains("boom") && m.contains('3')),
            "{err:?}"
        );
    }

    #[test]
    fn a_missing_program_is_an_error_naming_it() {
        let err = ProcessRunner
            .run("actautoma-no-such-tool", &[], None)
            .unwrap_err();
        assert!(
            matches!(&err, Error::Run(m) if m.contains("actautoma-no-such-tool not found")),
            "{err:?}"
        );
    }
}
