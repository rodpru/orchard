mod config;
mod error;
mod gh;
mod git;
mod process;
mod pty;
mod system;

use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    system::adopt_login_shell_path();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(pty::PtyState::default())
        .invoke_handler(tauri::generate_handler![
            pty::pty_attach,
            pty::pty_detach,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_kill,
            pty::pty_kill_worktrees,
            pty::pty_list,
            git::git_resolve_repo,
            git::git_list_worktrees,
            git::git_list_branches,
            git::git_create_worktree,
            git::git_remove_worktree,
            gh::gh_pr_list,
            config::config_load,
            config::config_save,
            system::open_in_editor,
            system::startup_path,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<pty::PtyState>().kill_all();
            }
        });
}
