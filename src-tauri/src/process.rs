use crate::error::{Error, Result};
use std::path::Path;
use std::process::Output;
use std::time::Duration;
use tokio::process::Command;

/// A git or gh call that hangs (a stuck credential helper, a dead network
/// mount) is killed after this, instead of holding a poll forever.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `program args` in `cwd` without blocking the async runtime and
/// returns its trimmed stdout. Failures name the command and exit status, so
/// an empty stderr never turns into an empty error. Gives up after `TIMEOUT`.
pub async fn run(cwd: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<String> {
    run_within(TIMEOUT, cwd, program, args, env).await
}

/// Like `run`, for commands that may legitimately take longer than `TIMEOUT`.
pub async fn run_within(
    timeout: Duration,
    cwd: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<String> {
    let output = output_within(timeout, cwd, program, args, env).await?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
    }
    Err(failure(program, args, &output))
}

/// Like `run`, but leaves the exit status to the caller, for programs whose
/// non-zero exits are answers (`git merge-tree` exits 1 on a conflict).
pub async fn output(cwd: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<Output> {
    output_within(TIMEOUT, cwd, program, args, env).await
}

/// The child is killed when the timeout drops it (`kill_on_drop`).
async fn output_within(
    timeout: Duration,
    cwd: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<Output> {
    let path = crate::system::path().await;
    let output = Command::new(program)
        .args(args)
        .env("PATH", path)
        .envs(env.iter().copied())
        .current_dir(cwd)
        .kill_on_drop(true)
        .output();
    tokio::time::timeout(timeout, output)
        .await
        .map_err(|_| Error::Invalid(format!("{program} {}: timed out after {}s", args.join(" "), timeout.as_secs())))?
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

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::async_runtime::block_on;

    #[test]
    fn a_hung_command_is_killed_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let start = std::time::Instant::now();
        let err = block_on(run_within(Duration::from_millis(200), dir.path(), "sleep", &["30"], &[])).unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
    }
}
