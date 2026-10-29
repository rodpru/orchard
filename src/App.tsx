import { ask, message, open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { NewWorktreeDialog, RemoveWorktreeDialog, SettingsDialog } from "./components/Dialogs";
import { Sidebar } from "./components/Sidebar";
import { WorktreeView } from "./components/WorktreeView";
import { useConfig } from "./hooks/useConfig";
import { useConflicts } from "./hooks/useConflicts";
import { usePrStatus } from "./hooks/usePrStatus";
import { useSessionIndicators } from "./hooks/useSessionIndicators";
import { useUpdate } from "./hooks/useUpdate";
import { useWorktrees } from "./hooks/useWorktrees";
import { api, errorMessage } from "./lib/api";
import { shortcutKey, shortcutLabel } from "./lib/platform";
import type { Config, SessionInfo, Worktree } from "./lib/types";
import "./styles.css";

type DialogState =
  | { kind: "new"; repo: string }
  | { kind: "remove"; repo: string; worktree: Worktree }
  | { kind: "settings" }
  | null;

const NO_SESSIONS: SessionInfo[] = [];

function withRepo(config: Config, repo: string): Config {
  return config.repos.includes(repo) ? config : { ...config, repos: [...config.repos, repo] };
}

export default function App() {
  const { t } = useTranslation();
  const { config, loadError, updateConfig } = useConfig();
  const repos = useMemo(() => config?.repos ?? [], [config]);
  const pinned = useMemo(() => config?.pinned ?? [], [config]);
  const { worktrees, repoErrors, refreshRepo, refreshAll, forgetRepo } = useWorktrees(repos);
  const { prs, prErrors, refreshPrs } = usePrStatus(repos, worktrees);
  const { clashes, conflictErrors, refreshConflicts } = useConflicts(repos, config?.fetchRemote ?? false);
  const [open, setOpen] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [agentOverrides, setAgentOverrides] = useState<Record<string, string>>({});
  const [restored, setRestored] = useState<Record<string, SessionInfo[]>>({});
  const { activity, unread, attention, onActivity, onBell, markSeen } = useSessionIndicators(selected);
  const [dialog, setDialog] = useState<DialogState>(null);
  const update = useUpdate(config?.autoUpdate ?? false);
  const { openReleases } = update;
  const openUpdate = useCallback(
    () =>
      void openReleases().catch(async (e) => {
        await message(errorMessage(e), { title: t("update.openFailed"), kind: "error" });
      }),
    [openReleases, t],
  );

  // Sessions outlive the webview, so a reload reopens the views it closed.
  useEffect(() => {
    api
      .ptyList()
      .then((sessions) => {
        const byCwd: Record<string, SessionInfo[]> = {};
        for (const session of sessions) (byCwd[session.cwd] ??= []).push(session);
        setRestored(byCwd);
        setOpen((current) => [...new Set([...current, ...Object.keys(byCwd)])]);
      })
      .catch(() => undefined);
  }, []);

  const byPath = useMemo(() => {
    const map: Record<string, { repo: string; worktree: Worktree }> = {};
    for (const [repo, list] of Object.entries(worktrees)) {
      for (const worktree of list) map[worktree.path] = { repo, worktree };
    }
    return map;
  }, [worktrees]);

  // Same order as the sidebar: pinned worktrees first, then repo by repo.
  const shortcutOrder = useMemo(() => {
    const rest = repos.flatMap((repo) => (worktrees[repo] ?? []).map((wt) => wt.path)).filter((p) => !pinned.includes(p));
    return [...pinned.filter((p) => p in byPath), ...rest].filter((p) => open.includes(p));
  }, [repos, worktrees, open, pinned, byPath]);

  const togglePin = useCallback(
    (path: string) =>
      void updateConfig((c) => ({
        ...c,
        pinned: c.pinned.includes(path) ? c.pinned.filter((p) => p !== path) : [...c.pinned, path],
      })),
    [updateConfig],
  );

  const select = useCallback(
    (path: string) => {
      setOpen((current) => (current.includes(path) ? current : [...current, path]));
      setSelected(path);
      markSeen(path);
    },
    [markSeen],
  );

  // `orchard <path>` opens the worktree containing <path>, registering its repo.
  const startupHandled = useRef(false);
  const loaded = config !== null;
  useEffect(() => {
    if (!loaded || startupHandled.current) return;
    startupHandled.current = true;
    void (async () => {
      const path = await api.startupPath();
      if (!path) return;
      try {
        const root = await api.resolveRepo(path);
        await updateConfig((c) => withRepo(c, root));
        const list = (await refreshRepo(root)) ?? [];
        const match = list
          .filter((wt) => path === wt.path || path.startsWith(`${wt.path}/`))
          .sort((a, b) => b.path.length - a.path.length)[0];
        if (match) select(match.path);
      } catch (e) {
        await message(errorMessage(e), { title: t("errors.notRepo"), kind: "error" });
      }
    })();
  }, [loaded, updateConfig, refreshRepo, select, t]);

  /** Closes the worktrees' views; resolves once their terminals have exited. */
  const closeSessions = useCallback(async (paths: string[]) => {
    setOpen((current) => current.filter((p) => !paths.includes(p)));
    setSelected((current) => (current && paths.includes(current) ? null : current));
    await api.ptyKillWorktrees(paths);
  }, []);

  const closeOne = useCallback((path: string) => void closeSessions([path]), [closeSessions]);

  const requestRemove = useCallback(
    (worktree: Worktree) => {
      const repo = byPath[worktree.path]?.repo;
      if (repo) setDialog({ kind: "remove", repo, worktree });
    },
    [byPath],
  );

  /**
   * Removes the worktree and, if asked, its local branch. The pin is dropped
   * whenever the worktree is gone afterwards, even when deleting the branch
   * failed.
   */
  const removeWorktree = async (repo: string, worktree: Worktree, options: { force: boolean; deleteBranch: boolean }) => {
    const { path } = worktree;
    await closeSessions([path]);
    try {
      await api.removeWorktree({
        repo,
        path,
        force: options.force,
        deleteBranch: options.deleteBranch ? worktree.branch : null,
      });
      setDialog(null);
    } finally {
      const list = await refreshRepo(repo);
      if (list && !list.some((wt) => wt.path === path)) {
        void updateConfig((c) => ({ ...c, pinned: c.pinned.filter((p) => p !== path) }));
      }
    }
  };

  const addRepo = async () => {
    const picked = await openDialog({ directory: true, multiple: false, title: t("sidebar.addRepo") });
    if (typeof picked !== "string") return;
    try {
      const root = await api.resolveRepo(picked);
      await updateConfig((c) => withRepo(c, root));
      await refreshRepo(root);
    } catch (e) {
      await message(errorMessage(e), { title: t("errors.notRepo"), kind: "error" });
    }
  };

  const removeRepo = async (repo: string) => {
    const confirmed = await ask(t("sidebar.removeRepoConfirm", { repo }), {
      title: t("sidebar.removeRepo"),
      kind: "warning",
      okLabel: t("common.remove"),
      cancelLabel: t("common.cancel"),
    });
    if (!confirmed) return;
    const paths = (worktrees[repo] ?? []).map((wt) => wt.path);
    void closeSessions(paths);
    forgetRepo(repo);
    await updateConfig((c) => ({
      ...c,
      repos: c.repos.filter((r) => r !== repo),
      pinned: c.pinned.filter((p) => !paths.includes(p)),
    }));
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const key = shortcutKey(event);
      if (key === null) return;
      if (/^[1-9]$/.test(key)) {
        const path = shortcutOrder[Number(key) - 1];
        if (path) {
          // Keep the keystroke away from the focused terminal.
          event.preventDefault();
          event.stopPropagation();
          select(path);
        }
      } else if (key === "n") {
        const repo = (selected && byPath[selected]?.repo) || repos[0];
        if (repo) {
          event.preventDefault();
          event.stopPropagation();
          setDialog({ kind: "new", repo });
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [shortcutOrder, select, selected, byPath, repos]);

  if (loadError) return <div className="fatal">{loadError}</div>;
  if (!config) return <div className="fatal">{t("app.loading")}</div>;

  return (
    <div className="app">
      <Sidebar
        repos={repos}
        worktrees={worktrees}
        repoErrors={repoErrors}
        prErrors={prErrors}
        prs={prs}
        clashes={clashes}
        conflictErrors={conflictErrors}
        pinned={pinned}
        open={open}
        shortcutOrder={shortcutOrder}
        selected={selected}
        activity={activity}
        unread={unread}
        attention={attention}
        onSelect={select}
        onTogglePin={togglePin}
        onNewWorktree={(repo) => setDialog({ kind: "new", repo })}
        onAddRepo={() => void addRepo()}
        onRemoveRepo={(repo) => void removeRepo(repo)}
        onRefresh={() => void refreshAll().then(() => Promise.all([refreshPrs(), refreshConflicts()]))}
        onSettings={() => setDialog({ kind: "settings" })}
        updateAvailable={update.available}
        onOpenUpdate={openUpdate}
      />

      <main className="main">
        {open.map((path) => {
          // A worktree missing from the last listing has no view; its
          // terminals keep running and reattach if it comes back.
          const worktree = byPath[path]?.worktree;
          if (!worktree) return null;
          return (
            <WorktreeView
              key={path}
              worktree={worktree}
              config={config}
              agentCommand={agentOverrides[path] ?? config.agentCommand}
              active={selected === path}
              pr={prs[path] ?? null}
              pinned={pinned.includes(path)}
              restored={restored[path] ?? NO_SESSIONS}
              onTogglePin={togglePin}
              onActivity={onActivity}
              onBell={onBell}
              onRemove={requestRemove}
              onClose={closeOne}
            />
          );
        })}

        {!(selected && selected in byPath) && (
          <div className="empty" data-tauri-drag-region>
            {repos.length === 0 ? (
              <>
                <h1>{t("empty.welcomeTitle")}</h1>
                <p>{t("empty.welcomeBody")}</p>
                <button className="primary" onClick={() => void addRepo()}>
                  {t("sidebar.addRepo")}
                </button>
              </>
            ) : (
              <>
                <h1>{t("empty.pickTitle")}</h1>
                <p>{t("empty.pickBody", { shortcut: shortcutLabel("n") })}</p>
              </>
            )}
          </div>
        )}
      </main>

      {dialog?.kind === "new" && (
        <NewWorktreeDialog
          repo={dialog.repo}
          config={config}
          onCancel={() => setDialog(null)}
          onCreated={async (path, agentCommand) => {
            const repo = dialog.repo;
            setDialog(null);
            setAgentOverrides((current) => ({ ...current, [path]: agentCommand }));
            await refreshRepo(repo);
            select(path);
          }}
        />
      )}

      {dialog?.kind === "remove" && (
        <RemoveWorktreeDialog
          worktree={dialog.worktree}
          onCancel={() => setDialog(null)}
          onConfirm={(options) => removeWorktree(dialog.repo, dialog.worktree, options)}
        />
      )}

      {dialog?.kind === "settings" && (
        <SettingsDialog
          config={config}
          updateError={update.error}
          onCancel={() => setDialog(null)}
          onSave={(draft) => {
            setDialog(null);
            // Only the settings fields: repos and pins may have changed meanwhile.
            void updateConfig((c) => ({
              ...c,
              agentCommand: draft.agentCommand,
              lazygitCommand: draft.lazygitCommand,
              editorCommand: draft.editorCommand,
              worktreeRoot: draft.worktreeRoot,
              language: draft.language,
              autoUpdate: draft.autoUpdate,
              fetchRemote: draft.fetchRemote,
            }));
          }}
        />
      )}
    </div>
  );
}
