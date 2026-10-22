import { Channel, invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl as openerOpenUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import type { SessionId } from "./sessions";
import type { Branches, Config, IpcError, LoadedConfig, PrSummary, PtyEvent, SessionInfo, Worktree } from "./types";

/** Terminal output arrives as raw bytes; everything else as a `PtyEvent`. */
export type PtyMessage = ArrayBuffer | PtyEvent;

export const api = {
  configLoad: () => invoke<LoadedConfig>("config_load"),
  configSave: (config: Config) => invoke<void>("config_save", { config }),

  resolveRepo: (path: string) => invoke<string>("git_resolve_repo", { path }),
  listWorktrees: (repo: string) => invoke<Worktree[]>("git_list_worktrees", { repo }),
  listBranches: (repo: string) => invoke<Branches>("git_list_branches", { repo }),
  createWorktree: (args: { repo: string; branch: string; base: string | null; root: string | null }) =>
    invoke<string>("git_create_worktree", args),
  /** Deletes only the local branch when `deleteBranch` is set; never the remote one. */
  removeWorktree: (args: { repo: string; path: string; force: boolean; deleteBranch: string | null }) =>
    invoke<void>("git_remove_worktree", args),

  /** PRs of the repo keyed by head branch. */
  prList: (repo: string) => invoke<Partial<Record<string, PrSummary>>>("gh_pr_list", { repo }),

  /** Starts the session if needed and streams its output; returns the token for `ptyDetach`. */
  ptyAttach: (
    args: { id: SessionId; cwd: string; command: string | null; cols: number; rows: number },
    onEvent: Channel<PtyMessage>,
  ) => invoke<number>("pty_attach", { ...args, onEvent }),
  ptyDetach: (id: SessionId, token: number) => invoke<void>("pty_detach", { id, token }),
  ptyWrite: (id: SessionId, data: string) => invoke<void>("pty_write", { id, data }),
  ptyResize: (id: SessionId, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  ptyKill: (id: SessionId) => invoke<void>("pty_kill", { id }),
  ptyKillWorktrees: (paths: string[]) => invoke<void>("pty_kill_worktrees", { paths }),
  ptyList: () => invoke<SessionInfo[]>("pty_list"),

  openInEditor: (editor: string, path: string) => invoke<void>("open_in_editor", { editor, path }),
  /** Whether the login shell finds the command's program; true when the check cannot tell. */
  commandExists: (command: string) => invoke<boolean>("command_exists", { command }),
  revealInFinder: (path: string) => revealItemInDir(path),
  openUrl: async (url: string) => {
    if (!isOpenableUrl(url)) throw new Error("only https URLs and http://localhost are allowed");
    await openerOpenUrl(url);
  },
  startupPath: () => invoke<string | null>("startup_path"),

  readClipboard: () => readText(),
  writeClipboard: (text: string) => writeText(text),
};

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

/** Any https URL, plus plain http to this machine (local dev servers). */
export function isOpenableUrl(url: string): boolean {
  try {
    const { protocol, hostname } = new URL(url);
    return protocol === "https:" || (protocol === "http:" && LOCAL_HOSTS.has(hostname));
  } catch {
    return false;
  }
}

function isIpcError(e: unknown): e is IpcError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e && typeof e.message === "string";
}

/** Readable text for anything a command or plugin call can reject with. */
export function errorMessage(e: unknown): string {
  if (isIpcError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

export function basename(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}
