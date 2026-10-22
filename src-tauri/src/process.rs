use crate::error::{Error, Result};
use std::path::Path;
use tokio::process::Command;

/// Runs `program args` in `cwd` without blocking the async runtime and
/// returns its trimmed stdout. Failures name the command and exit status, so
/// an empty stderr never turns into an empty error.
pub async fn run(cwd: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .env("PATH", crate::system::path())
        .envs(env.iter().copied())
        .current_dir(cwd)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|source| Error::Spawn { program: program.to_owned(), source })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(Error::Command {
        command: format!("{program} {}", args.join(" ")),
        code: output.status.code().map_or_else(|| "signal".into(), |c| c.to_string()),
        stderr: if stderr.is_empty() { "(no output)".into() } else { stderr },
    })
}
