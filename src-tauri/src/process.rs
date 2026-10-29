use crate::error::{Error, Result};
use std::path::Path;
use std::process::Output;
use tokio::process::Command;

/// Runs `program args` in `cwd` without blocking the async runtime and
/// returns its trimmed stdout. Failures name the command and exit status, so
/// an empty stderr never turns into an empty error.
pub async fn run(cwd: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<String> {
    let output = output(cwd, program, args, env).await?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
    }
    Err(failure(program, args, &output))
}

/// Like `run`, but leaves the exit status to the caller, for programs whose
/// non-zero exits are answers (`git merge-tree` exits 1 on a conflict).
pub async fn output(cwd: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<Output> {
    let path = crate::system::path().await;
    Command::new(program)
        .args(args)
        .env("PATH", path)
        .envs(env.iter().copied())
        .current_dir(cwd)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|source| Error::Spawn { program: program.to_owned(), source })
}

/// The error for a program that ran and exited with a status the caller did not expect.
pub fn failure(program: &str, args: &[&str], output: &Output) -> Error {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Error::Command {
        command: format!("{program} {}", args.join(" ")),
        code: output.status.code().map_or_else(|| "signal".into(), |c| c.to_string()),
        stderr: if stderr.is_empty() { "(no output)".into() } else { stderr },
    }
}
