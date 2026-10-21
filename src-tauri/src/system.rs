use crate::error::{Error, Result};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// A slow or hung shell rc file must not keep the window from opening.
const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(2);

/// GUI apps on macOS start with a minimal PATH, so tools installed via
/// Homebrew, nvm, cargo, etc. are invisible. Copy the login shell's PATH.
pub fn adopt_login_shell_path() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let Ok(mut child) = Command::new(&shell)
        .args(["-l", "-i", "-c", "printf '__ORCHARD__%s__ORCHARD__' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let Some(mut stdout) = child.stdout.take() else { return };
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let Ok(out) = rx.recv_timeout(SHELL_PATH_TIMEOUT) else {
        // Killing the shell closes the pipe, so the reader thread ends too.
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        return;
    };
    let _ = child.wait();
    let _ = reader.join();
    if let Some(path) = out.split("__ORCHARD__").nth(1).filter(|p| !p.is_empty()) {
        // SAFETY: called once at startup; the only other thread was joined above.
        unsafe { std::env::set_var("PATH", path) };
    }
}

/// Splits the configured editor like a shell would, so `code --wait` or
/// `cursor -n` work, then appends the worktree path as the last argument.
fn editor_argv(editor: &str, path: &str) -> Result<Vec<String>> {
    let mut argv = shell_words::split(editor.trim()).map_err(|e| Error::Invalid(format!("editor command: {e}")))?;
    if argv.is_empty() {
        return Err(Error::Invalid("editor command is empty".into()));
    }
    argv.push(path.to_owned());
    Ok(argv)
}

#[tauri::command]
pub async fn open_in_editor(editor: String, path: String) -> Result<()> {
    let argv = editor_argv(&editor, &path)?;
    // tokio reaps the child in the background, so no zombie is left behind.
    tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|source| Error::Spawn { program: argv[0].clone(), source })
}

/// First positional CLI argument as an absolute path, so `orchard .` from a
/// terminal opens that worktree.
#[tauri::command]
pub fn startup_path() -> Option<String> {
    let arg = std::env::args().skip(1).find(|a| !a.starts_with('-'))?;
    std::fs::canonicalize(arg).ok().map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_arguments_are_split() {
        assert_eq!(editor_argv("code --wait", "/a b").unwrap(), ["code", "--wait", "/a b"]);
        assert_eq!(editor_argv("'/Applications/My Editor' -n", "/p").unwrap(), ["/Applications/My Editor", "-n", "/p"]);
        assert!(editor_argv("  ", "/p").is_err());
    }
}
