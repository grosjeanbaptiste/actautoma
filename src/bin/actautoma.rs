//! `actautoma rename-repo OWNER/OLD NEW [options]`: see `actautoma::cli`.

use actautoma::cli::dispatch;
use actautoma::gh::Gh;
use actautoma::process::ProcessRunner;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = |key: &str| std::env::var(key).ok();
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("actautoma: no current folder: {e}");
            return ExitCode::FAILURE;
        }
    };
    let forge = Gh::new(ProcessRunner);
    match dispatch(
        &args,
        &cwd,
        &env,
        &forge,
        &ProcessRunner,
        &mut std::io::stdout().lock(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("actautoma: {e}");
            ExitCode::FAILURE
        }
    }
}
