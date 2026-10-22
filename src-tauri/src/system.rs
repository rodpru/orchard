use crate::error::{Error, Result};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

/// A slow or hung shell rc file must not keep programs from starting forever.
const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

static PATH: OnceLock<OsString> = OnceLock::new();

/// The user's shell; `/bin/sh` when `SHELL` is unset, since not every Linux
/// distribution ships zsh.
pub fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

/// GUI apps launched from the Dock or a desktop launcher start with a minimal
/// PATH, so tools installed via Homebrew, nvm, cargo, ~/.local/bin, etc. are
/// invisible. Asks the login shell for its PATH; None when it fails or hangs.
fn login_shell_path() -> Option<String> {
    let shell = user_shell();
    let mut child = Command::new(&shell)
        .args(["-l", "-i", "-c", "printf '__ORCHARD__%s__ORCHARD__' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let out = rx.recv_timeout(SHELL_PATH_TIMEOUT).ok();
    if out.is_none() {
        // Killing the shell closes the pipe, so the reader thread ends too.
        let _ = child.kill();
    }
    let _ = child.wait();
    let _ = reader.join();
    out?.split("__ORCHARD__").nth(1).filter(|p| !p.is_empty()).map(str::to_owned)
}

/// Starts resolving the PATH for spawned programs on a background thread, so
/// a slow shell rc file delays the first git call instead of the window.
pub fn warm_path() {
    std::thread::spawn(|| {
        path();
    });
}

/// PATH for every program Orchard starts: the login shell's, or this
/// process's when the shell fails. The first call blocks until the shell has
/// answered or timed out; later calls are free.
pub fn path() -> &'static OsStr {
    PATH.get_or_init(|| login_shell_path().map(OsString::from).or_else(|| std::env::var_os("PATH")).unwrap_or_default())
}

/// WebKitGTK's DMA-BUF renderer shows a blank window on some NVIDIA and
/// Wayland setups. Turn it off unless the user chose otherwise.
#[cfg(target_os = "linux")]
pub fn configure_webkit() {
    const VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    if std::env::var_os(VAR).is_none() {
        // SAFETY: called at startup, before any other thread exists.
        unsafe { std::env::set_var(VAR, "1") };
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
        .env("PATH", self::path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|source| Error::Spawn { program: argv[0].clone(), source })
}

/// The program a configured command starts: its first word, split like a shell would.
fn command_program(command: &str) -> Option<String> {
    shell_words::split(command.trim()).ok()?.into_iter().next()
}

/// Whether the login shell can run `command`, checked the same way the
/// terminal panes start it, so aliases and shell functions count. Anything
/// that keeps the check from answering counts as found: the pane then shows
/// the shell's own error instead of a wrong "not installed" notice.
#[tauri::command]
pub async fn command_exists(command: String) -> bool {
    let Some(program) = command_program(&command) else { return true };
    let check = format!("command -v -- {} >/dev/null", shell_words::quote(&program));
    let status = tokio::process::Command::new(user_shell())
        .args(["-l", "-i", "-c", &check])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .status();
    match tokio::time::timeout(COMMAND_CHECK_TIMEOUT, status).await {
        Ok(Ok(status)) => status.success(),
        _ => true,
    }
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

    #[test]
    fn command_program_is_the_first_word() {
        assert_eq!(command_program("lazygit").as_deref(), Some("lazygit"));
        assert_eq!(command_program("  lazygit --use-config-file x.yml").as_deref(), Some("lazygit"));
        assert_eq!(command_program("'/opt/my tools/lazygit' -p .").as_deref(), Some("/opt/my tools/lazygit"));
        assert_eq!(command_program("   "), None);
        assert_eq!(command_program("'unclosed"), None);
    }

    #[test]
    fn path_is_resolved_once_and_never_empty() {
        let first = path();
        assert!(!first.is_empty());
        assert!(std::ptr::eq(first, path()));
    }
}
