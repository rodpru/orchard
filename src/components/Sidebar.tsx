import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { basename } from "../lib/api";
import { shortcutBadge, shortcutLabel } from "../lib/platform";
import type { PrSummary, Worktree } from "../lib/types";
import { PrBadge } from "./PrBadge";

const BUSY_WINDOW_MS = 3000;

interface Props {
  repos: string[];
  worktrees: Record<string, Worktree[]>;
  repoErrors: Record<string, string>;
  prErrors: Record<string, string>;
  prs: Record<string, PrSummary | null>;
  pinned: string[];
  open: string[];
  shortcutOrder: string[];
  selected: string | null;
  activity: Record<string, number>;
  unread: Set<string>;
  attention: Set<string>;
  onSelect: (path: string) => void;
  onTogglePin: (path: string) => void;
  onNewWorktree: (repo: string) => void;
  onAddRepo: () => void;
  onRemoveRepo: (repo: string) => void;
  onRefresh: () => void;
  onSettings: () => void;
  /** Linux: a newer release the user installs from the release page. */
  updateAvailable: string | null;
  onOpenUpdate: () => void;
}

export function PinIcon({ filled }: { filled: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <path
        d="M10.5 1.5l4 4-2 1-2.5 2.5.5 3.5-1.5 1.5-3-3-4 4-1-1 4-4-3-3L4 5.5l3.5.5L10 3.5z"
        fill={filled ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function Sidebar(props: Props) {
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const toggle = (key: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const pinnedSet = new Set(props.pinned);
  const pinnedRows = props.pinned.flatMap((path) => {
    for (const repo of props.repos) {
      const wt = (props.worktrees[repo] ?? []).find((w) => w.path === path);
      if (wt) return [{ repo, wt }];
    }
    return [];
  });

  const statusClass = (path: string) => {
    const isOpen = props.open.includes(path);
    if (props.attention.has(path)) return "attention";
    if (isOpen && now - (props.activity[path] ?? 0) < BUSY_WINDOW_MS) return "busy";
    if (props.unread.has(path)) return "unread";
    return isOpen ? "running" : "";
  };

  const renderRow = (wt: Worktree, repo: string | null) => {
    const pr = props.prs[wt.path];
    const index = props.shortcutOrder.indexOf(wt.path) + 1;
    const isPinned = pinnedSet.has(wt.path);
    return (
      <div
        key={wt.path}
        role="button"
        tabIndex={0}
        className={`worktree-row ${props.selected === wt.path ? "selected" : ""} ${isPinned ? "pinned" : ""}`}
        onClick={() => props.onSelect(wt.path)}
        onKeyDown={(e) => e.key === "Enter" && props.onSelect(wt.path)}
        title={wt.path}
      >
        <span className={`status-dot ${statusClass(wt.path)}`} />
        <span className="worktree-name">
          {wt.isMain && <span className="main-tag">{t("sidebar.main")}</span>}
          {wt.branch ?? t("worktree.detached")}
          {repo && <span className="worktree-repo">{basename(repo)}</span>}
        </span>
        <span className="worktree-meta">
          {wt.changes > 0 && <span className="changes-count">±{wt.changes}</span>}
          {pr && <PrBadge summary={pr} compact />}
          {index > 0 && index < 10 && <kbd>{shortcutBadge(String(index))}</kbd>}
          <button
            className="pin-button"
            title={isPinned ? t("sidebar.unpin") : t("sidebar.pin")}
            aria-label={isPinned ? t("sidebar.unpin") : t("sidebar.pin")}
            onClick={(e) => {
              e.stopPropagation();
              props.onTogglePin(wt.path);
            }}
          >
            <PinIcon filled={isPinned} />
          </button>
        </span>
      </div>
    );
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-header" data-tauri-drag-region>
        <span className="app-name" data-tauri-drag-region>
          {t("app.name")}
        </span>
        <div className="sidebar-header-actions">
          <button className="icon" onClick={props.onRefresh} title={t("sidebar.refresh")}>
            ↻
          </button>
          <button className="icon" onClick={props.onSettings} title={t("sidebar.settings")}>
            ⚙
          </button>
        </div>
      </div>

      <div className="repo-list">
        {pinnedRows.length > 0 && (
          <div className="repo">
            <div className="repo-row">
              <span className="repo-name static">{t("sidebar.pinned")}</span>
            </div>
            {pinnedRows.map(({ repo, wt }) => renderRow(wt, repo))}
          </div>
        )}

        {props.repos.map((repo) => {
          const isCollapsed = collapsed.has(repo);
          const rows = (props.worktrees[repo] ?? []).filter((wt) => !pinnedSet.has(wt.path));
          return (
            <div key={repo} className="repo">
              <div className="repo-row">
                <button className="repo-name" onClick={() => toggle(repo)} title={repo}>
                  <span className="chevron">{isCollapsed ? "▸" : "▾"}</span>
                  {basename(repo)}
                </button>
                <button className="icon" onClick={() => props.onNewWorktree(repo)} title={t("sidebar.newWorktree", { shortcut: shortcutLabel("n") })}>
                  +
                </button>
                <button className="icon subtle" onClick={() => props.onRemoveRepo(repo)} title={t("sidebar.removeRepo")}>
                  −
                </button>
              </div>
              {props.repoErrors[repo] && <p className="error small">{props.repoErrors[repo]}</p>}
              {props.prErrors[repo] && (
                <p className="warning small" title={props.prErrors[repo]}>
                  {t("sidebar.prError", { error: props.prErrors[repo] })}
                </p>
              )}
              {!isCollapsed && rows.map((wt) => renderRow(wt, null))}
            </div>
          );
        })}
      </div>

      <div className="sidebar-footer">
        {props.updateAvailable && (
          <button className="primary" onClick={props.onOpenUpdate} title={t("update.availableHint")}>
            {t("update.available", { version: props.updateAvailable })}
          </button>
        )}
        <button onClick={props.onAddRepo}>{t("sidebar.addRepo")}</button>
      </div>
    </aside>
  );
}
