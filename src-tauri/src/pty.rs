//! Terminal sessions are owned here, not by the webview. A view attaches to a
//! session to receive its output and detaches when it unmounts; the process
//! keeps running and its recent output is replayed on the next attach. Only an
//! explicit kill (closing a tab or worktree, quitting the app) ends a process.
//!
//! Most commands are sync on purpose: Tauri runs them on the main thread in
//! arrival order, which keeps keystrokes ordered. None of them block: input
//! goes through a per-session writer thread, output through a per-session
//! reader thread, and kills finish on a background thread. Only
//! `pty_kill_worktrees` is async, because callers wait for the processes to exit.

use crate::error::{Error, Result};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use ts_rs::TS;

/// Output kept per session for replay, roughly 10k lines of agent output.
const SCROLLBACK_BYTES: usize = 2 * 1024 * 1024;
/// How long a process gets to exit after SIGHUP before it is SIGKILLed.
const KILL_GRACE: Duration = Duration::from_secs(1);
const QUIT_GRACE: Duration = Duration::from_millis(300);

/// Non-output events. Output itself is sent as raw bytes (an `ArrayBuffer`
/// in the webview), which avoids escaping every chunk into JSON.
#[derive(Serialize, TS)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
#[ts(export)]
pub enum PtyEvent {
    Exit(Option<u32>),
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionInfo {
    id: String,
    cwd: String,
    command: Option<String>,
    exited: bool,
}

#[derive(Default)]
struct Output {
    scrollback: VecDeque<u8>,
    /// Attach token and channel of the view currently showing the session.
    subscriber: Option<(u64, Channel<InvokeResponseBody>)>,
    /// `Some(code)` once the process has exited.
    exit: Option<Option<u32>>,
}

impl Output {
    fn push(&mut self, bytes: &[u8]) {
        self.scrollback.extend(bytes);
        if self.scrollback.len() > SCROLLBACK_BYTES {
            self.scrollback.drain(..self.scrollback.len() - SCROLLBACK_BYTES);
            // Never start the replay in the middle of a UTF-8 character.
            while self.scrollback.front().is_some_and(|b| b & 0b1100_0000 == 0b1000_0000) {
                self.scrollback.pop_front();
            }
        }
        self.send(InvokeResponseBody::Raw(bytes.to_vec()));
    }

    fn send(&mut self, body: InvokeResponseBody) {
        // A closed channel (webview reloaded) only drops the subscriber: the
        // process keeps running and the next attach replays what it missed.
        if let Some((_, channel)) = &self.subscriber
            && channel.send(body).is_err()
        {
            self.subscriber = None;
        }
    }
}

struct Session {
    cwd: String,
    command: Option<String>,
    pid: Option<u32>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    input: mpsc::Sender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    output: Mutex<Output>,
}

impl Session {
    fn exited(&self) -> bool {
        self.output.lock().unwrap().exit.is_some()
    }

    /// SIGHUP, the signal a closing terminal sends. Skipped once the process
    /// has been reaped so a recycled pid is never signalled.
    fn hang_up(&self) {
        if !self.exited() {
            let _ = self.killer.lock().unwrap().kill();
        }
    }

    fn force_kill(&self) {
        if let Some(pid) = self.pid.filter(|_| !self.exited()) {
            // SAFETY: plain syscall on a pid we spawned and have not reaped.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

/// Hangs up every session, waits up to `grace` for them to exit, then
/// SIGKILLs whatever is left.
fn terminate(sessions: &[Arc<Session>], grace: Duration) {
    sessions.iter().for_each(|s| s.hang_up());
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline && sessions.iter().any(|s| !s.exited()) {
        std::thread::sleep(Duration::from_millis(25));
    }
    sessions.iter().for_each(|s| s.force_kill());
}

#[derive(Default)]
pub struct PtyState {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    next_token: AtomicU64,
}

impl PtyState {
    fn get(&self, id: &str) -> Result<Arc<Session>> {
        self.sessions.lock().unwrap().get(id).cloned().ok_or_else(|| Error::Pty(format!("session not found: {id}")))
    }

    /// Removes the matching sessions under the lock and returns them, so the
    /// kill itself never holds the lock.
    fn take_where(&self, matches: impl Fn(&str, &Session) -> bool) -> Vec<Arc<Session>> {
        let mut sessions = self.sessions.lock().unwrap();
        let ids: Vec<_> = sessions.iter().filter(|(id, s)| matches(id, s)).map(|(id, _)| id.clone()).collect();
        ids.iter().filter_map(|id| sessions.remove(id)).collect()
    }

    /// Called when the app quits; blocks briefly so no process outlives it.
    pub fn kill_all(&self) {
        let all: Vec<_> = self.sessions.lock().unwrap().drain().map(|(_, s)| s).collect();
        terminate(&all, QUIT_GRACE);
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

fn spawn(cwd: &str, command: Option<&str>, cols: u16, rows: u16) -> Result<Arc<Session>> {
    let pair = native_pty_system().openpty(pty_size(cols, rows)).map_err(Error::pty)?;

    let shell = crate::system::user_shell();
    let mut cmd = CommandBuilder::new(&shell);
    match command {
        // Run the command inside a login shell, then fall back to an
        // interactive shell so quitting the tool does not kill the pane.
        Some(c) => cmd.args(["-l", "-i", "-c", &format!("{c}; exec {shell} -l -i")]),
        None => cmd.args(["-l", "-i"]),
    }
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "orchard");
    if std::env::var("LANG").is_err() {
        cmd.env("LANG", "en_US.UTF-8");
    }

    let mut child = pair.slave.spawn_command(cmd).map_err(Error::pty)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(Error::pty)?;
    let mut writer = pair.master.take_writer().map_err(Error::pty)?;

    let (input, keystrokes) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        // Blocks only this thread when the program stops reading its input.
        for data in keystrokes {
            if writer.write_all(&data).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });

    let session = Arc::new(Session {
        cwd: cwd.to_owned(),
        command: command.map(str::to_owned),
        pid: child.process_id(),
        master: Mutex::new(pair.master),
        input,
        killer: Mutex::new(child.clone_killer()),
        output: Mutex::new(Output::default()),
    });

    let reading = Arc::clone(&session);
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => reading.output.lock().unwrap().push(&buf[..n]),
            }
        }
        let code = child.wait().ok().map(|status| status.exit_code());
        let mut output = reading.output.lock().unwrap();
        output.exit = Some(code);
        if let Ok(event) = serde_json::to_string(&PtyEvent::Exit(code)) {
            output.send(InvokeResponseBody::Json(event));
        }
    });

    Ok(session)
}

/// Shows session `id` through `on_event`, starting it first if it does not
/// exist. The returned token identifies this attachment for `pty_detach`.
#[tauri::command]
pub fn pty_attach(
    state: State<'_, PtyState>,
    id: String,
    cwd: String,
    command: Option<String>,
    cols: u16,
    rows: u16,
    on_event: Channel<InvokeResponseBody>,
) -> Result<u64> {
    let existing = state.sessions.lock().unwrap().get(&id).cloned();
    let session = match existing {
        Some(session) => {
            let _ = session.master.lock().unwrap().resize(pty_size(cols, rows));
            session
        }
        None => {
            let command = command.as_deref().map(str::trim).filter(|c| !c.is_empty());
            let session = spawn(&cwd, command, cols, rows)?;
            state.sessions.lock().unwrap().insert(id, Arc::clone(&session));
            session
        }
    };

    let token = state.next_token.fetch_add(1, Ordering::Relaxed);
    let mut output = session.output.lock().unwrap();
    if !output.scrollback.is_empty() {
        let replay = output.scrollback.iter().copied().collect();
        let _ = on_event.send(InvokeResponseBody::Raw(replay));
    }
    if let Some(code) = output.exit {
        let _ = on_event.send(InvokeResponseBody::Json(serde_json::to_string(&PtyEvent::Exit(code))?));
    }
    output.subscriber = Some((token, on_event));
    Ok(token)
}

/// Stops sending output to the view that attached with `token`. A newer
/// attachment (StrictMode remount, another view) is left alone.
#[tauri::command]
pub fn pty_detach(state: State<'_, PtyState>, id: String, token: u64) {
    if let Ok(session) = state.get(&id) {
        let mut output = session.output.lock().unwrap();
        if output.subscriber.as_ref().is_some_and(|(t, _)| *t == token) {
            output.subscriber = None;
        }
    }
}

#[tauri::command]
pub fn pty_write(state: State<'_, PtyState>, id: String, data: String) -> Result<()> {
    state.get(&id)?.input.send(data.into_bytes()).map_err(|_| Error::Pty(format!("session closed: {id}")))
}

#[tauri::command]
pub fn pty_resize(state: State<'_, PtyState>, id: String, cols: u16, rows: u16) -> Result<()> {
    let session = state.get(&id)?;
    let master = session.master.lock().unwrap();
    master.resize(pty_size(cols, rows)).map_err(Error::pty)
}

/// Kills one session. Sync, so an attach sent right after it (restarting an
/// exited terminal) starts a new process; the kill finishes in the background.
#[tauri::command]
pub fn pty_kill(state: State<'_, PtyState>, id: String) {
    let removed = state.take_where(|session_id, _| session_id == id);
    if !removed.is_empty() {
        std::thread::spawn(move || terminate(&removed, KILL_GRACE));
    }
}

/// Kills every session running in one of `paths` (closing or removing
/// worktrees, removing a repo) and resolves once they have exited, so a
/// following `git worktree remove` does not race the processes in it.
#[tauri::command]
pub async fn pty_kill_worktrees(state: State<'_, PtyState>, paths: Vec<String>) -> Result<()> {
    let removed = state.take_where(|_, session| paths.contains(&session.cwd));
    tauri::async_runtime::spawn_blocking(move || terminate(&removed, KILL_GRACE)).await.map_err(Error::pty)
}

/// Live sessions, so a reloaded webview can reopen the views it lost.
#[tauri::command]
pub fn pty_list(state: State<'_, PtyState>) -> Vec<SessionInfo> {
    state
        .sessions
        .lock()
        .unwrap()
        .iter()
        .map(|(id, s)| SessionInfo {
            id: id.clone(),
            cwd: s.cwd.clone(),
            command: s.command.clone(),
            exited: s.exited(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(session: &Session, what: impl Fn(&Output) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !what(&session.output.lock().unwrap()) {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn text(output: &Output) -> String {
        String::from_utf8_lossy(&output.scrollback.iter().copied().collect::<Vec<_>>()).into_owned()
    }

    #[test]
    fn scrollback_is_capped_on_a_char_boundary() {
        let mut output = Output::default();
        output.push(&vec![b'a'; SCROLLBACK_BYTES - 1]);
        output.push("é!".as_bytes()); // 2-byte char straddles the cap
        output.push(b"z");
        assert!(output.scrollback.len() <= SCROLLBACK_BYTES);
        assert_ne!(output.scrollback.front().unwrap() & 0b1100_0000, 0b1000_0000);
    }

    #[test]
    fn output_is_kept_without_a_view_and_kill_ends_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().into_owned();
        let session = spawn(&cwd, Some("echo orchard-$((40+2))"), 80, 24).unwrap();
        wait_for(&session, |o| text(o).contains("orchard-42"));

        session.input.send(b"echo typed-$((1+1))\r".to_vec()).unwrap();
        wait_for(&session, |o| text(o).contains("typed-2"));

        terminate(&[Arc::clone(&session)], KILL_GRACE);
        wait_for(&session, |o| o.exit.is_some());
    }
}
