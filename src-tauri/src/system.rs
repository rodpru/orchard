use crate::error::{Error, Result};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

/// A slow or hung shell rc file must not keep programs from starting forever.
const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const MARKER: &str = "__ORCHARD__";

static PATH: OnceLock<OsString> = OnceLock::new();

/// The user's shell; `/bin/sh` when `SHELL` is unset, since not every Linux
/// distribution ships zsh.
pub fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

/// The text between the first two markers, once both have arrived.
fn between_markers(out: &[u8]) -> Option<String> {
    let out = String::from_utf8_lossy(out);
    let mut parts = out.split(MARKER).skip(1);
    let path = parts.next()?;
    parts.next()?;
    Some(path.to_owned())
}

/// GUI apps launched from the Dock or a desktop launcher start with a minimal
/// PATH, so tools installed via Homebrew, nvm, cargo, ~/.local/bin, etc. are
/// invisible. Asks `shell` for its login PATH; None when it fails or hangs.
fn probe_path(shell: &str, timeout: Duration) -> Option<String> {
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // Its own process group, so the kill below also reaches whatever the
        // rc files started.
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    // Reads up to the closing marker, not to EOF: a daemon started by an rc
    // file (`eval $(ssh-agent)`, gpg-agent) can hold the pipe open forever.
    // For the same reason the thread is never joined; it ends with the pipe.
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = stdout.read(&mut buf) {
            out.extend_from_slice(&buf[..n]);
            if let Some(path) = between_markers(&out) {
                let _ = tx.send(path);
                return;
            }
        }
    });
    let path = rx.recv_timeout(timeout).ok();
    // SAFETY: plain syscall on the group created above; the shell is not
    // reaped yet, so its pid, and with it the group id, cannot be reused.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.wait();
    path.filter(|p| !p.is_empty())
}

/// PATH for programs Orchard starts: the login shell's, or this process's
/// when the shell fails. Blocks until the probe has answered or timed out.
fn resolved_path() -> &'static OsStr {
    PATH.get_or_init(|| {
        probe_path(&user_shell(), SHELL_PATH_TIMEOUT)
            .map(OsString::from)
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default()
    })
}

/// Starts the PATH probe on a background thread, so a slow shell rc file
/// delays the first git call instead of the window.
pub fn warm_path() {
    std::thread::spawn(resolved_path);
}

/// The resolved PATH, waiting for the probe on a blocking thread rather than
/// on the caller's async worker.
pub async fn path() -> &'static OsStr {
    match PATH.get() {
        Some(path) => path,
        None => tauri::async_runtime::spawn_blocking(resolved_path).await.unwrap_or_default(),
    }
}

/// The resolved PATH if the probe has already answered.
pub fn ready_path() -> Option<&'static OsStr> {
    PATH.get().map(OsString::as_os_str)
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
    let search_path = self::path().await;
    // tokio reaps the child in the background, so no zombie is left behind.
    tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .env("PATH", search_path)
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
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

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

    /// A stand-in for the user's shell: ignores its arguments and runs `body`.
    fn fake_shell(dir: &tempfile::TempDir, body: &str) -> String {
        let path = dir.path().join("shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_daemon_holding_stdout_does_not_hang_the_probe() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(&dir, "sleep 30 &\nprintf '__ORCHARD__/fake/bin__ORCHARD__'");
        let start = Instant::now();
        assert_eq!(probe_path(&shell, Duration::from_secs(5)).as_deref(), Some("/fake/bin"));
        assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
    }

    #[test]
    fn a_hung_shell_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(&dir, "sleep 30 &\nsleep 30");
        let start = Instant::now();
        assert_eq!(probe_path(&shell, Duration::from_millis(200)), None);
        assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
    }

    #[test]
    fn concurrent_callers_share_one_resolved_path() {
        let threads: Vec<_> = (0..8).map(|_| std::thread::spawn(resolved_path)).collect();
        let first = resolved_path();
        assert!(!first.is_empty());
        for thread in threads {
            assert!(std::ptr::eq(thread.join().unwrap(), first));
        }
        assert!(std::ptr::eq(tauri::async_runtime::block_on(path()), first));
        assert!(ready_path().is_some_and(|p| std::ptr::eq(p, first)));
    }
}
