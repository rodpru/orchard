mod config;
mod conflicts;
mod error;
mod gh;
mod git;
mod process;
mod pty;
mod system;
mod update;

use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    system::configure_webkit();
    let context = tauri::generate_context!();
    system::warm_path();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(pty::PtyState::default())
        .manage(update::UpdateState::default())
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
            conflicts::conflicts_check,
            config::config_load,
            config::config_save,
            system::open_in_editor,
            system::command_exists,
            system::startup_path,
            update::update_check,
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<pty::PtyState>().kill_all();
            }
        });
}
