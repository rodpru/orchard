import { message } from "@tauri-apps/plugin-dialog";
import { memo, useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, basename, errorMessage } from "../lib/api";
import { type SessionId, sessionId, shellNumber } from "../lib/sessions";
import type { Config, PrSummary, SessionInfo, Worktree } from "../lib/types";
import { PrBadge } from "./PrBadge";
import { PinIcon } from "./Sidebar";
import { Terminal } from "./Terminal";

interface Tab {
  id: SessionId;
  kind: "agent" | "shell";
  command: string | null;
  label: string;
}

interface Props {
  worktree: Worktree;
  config: Config;
  agentCommand: string;
  active: boolean;
  pr: PrSummary | null;
  pinned: boolean;
  /** Shell sessions still running from before a webview reload. */
  restored: SessionInfo[];
  onTogglePin: (path: string) => void;
  onActivity: (path: string) => void;
  onBell: (path: string) => void;
  onRemove: (worktree: Worktree) => void;
  onClose: (path: string) => void;
}

const CREATE_PR_COMMAND = "git push -u origin HEAD && gh pr create --web";

const showError = (e: unknown) => void message(errorMessage(e), { kind: "error" });

function initialTabs(path: string, agentCommand: string, restored: SessionInfo[], shellLabel: (n: number) => string) {
  const shells = restored
    .flatMap((session) => {
      const n = shellNumber(session.id, path);
      return n === null ? [] : [{ n, session }];
    })
    .sort((a, b) => a.n - b.n);
  const tabs: Tab[] = [
    { id: sessionId.agent(path), kind: "agent", command: agentCommand || null, label: agentCommand || "shell" },
    ...shells.map(({ n, session }): Tab => ({
      id: sessionId.shell(path, n),
      kind: "shell",
      command: session.command,
      label: shellLabel(n),
    })),
  ];
  return { tabs, shellCount: Math.max(0, ...shells.map(({ n }) => n)) };
}

function WorktreeViewImpl({ worktree, config, agentCommand, active, pr, pinned, restored, onTogglePin, onActivity, onBell, onRemove, onClose }: Props) {
  const { t } = useTranslation();
  const { path } = worktree;
  const [initial] = useState(() => initialTabs(path, agentCommand, restored, (n) => t("worktree.shellTab", { n })));
  const shellCount = useRef(initial.shellCount);
  const [tabs, setTabs] = useState<Tab[]>(initial.tabs);
  const [activeTab, setActiveTab] = useState(tabs[0].id);
  const [showGit, setShowGit] = useState(true);
  const [split, setSplit] = useState(58);
  const bodyRef = useRef<HTMLDivElement>(null);

  const addShell = useCallback(
    (command: string | null = null, label?: string) => {
      shellCount.current += 1;
      const tab: Tab = {
        id: sessionId.shell(path, shellCount.current),
        kind: "shell",
        command,
        label: label ?? t("worktree.shellTab", { n: shellCount.current }),
      };
      setTabs((current) => [...current, tab]);
      setActiveTab(tab.id);
    },
    [path, t],
  );

  const closeTab = (id: SessionId) => {
    void api.ptyKill(id);
    const next = tabs.filter((tab) => tab.id !== id);
    setTabs(next);
    if (activeTab === id) setActiveTab(next[next.length - 1].id);
  };

  const startDrag = (event: React.MouseEvent) => {
    event.preventDefault();
    const body = bodyRef.current;
    if (!body) return;
    const rect = body.getBoundingClientRect();
    const move = (e: MouseEvent) => {
      const percent = ((e.clientX - rect.left) / rect.width) * 100;
      setSplit(Math.min(80, Math.max(25, percent)));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("dragging");
    };
    document.body.classList.add("dragging");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const activity = useCallback(() => onActivity(path), [onActivity, path]);
  const bell = useCallback(() => onBell(path), [onBell, path]);

  return (
    <section className="worktree-view" style={{ display: active ? "flex" : "none" }}>
      <header className="worktree-header" data-tauri-drag-region>
        <div className="worktree-title" data-tauri-drag-region>
          <span className="branch">{worktree.branch ?? t("worktree.detached")}</span>
          <span className="path" title={path}>
            {basename(path)}
          </span>
          {worktree.changes > 0 && (
            <span className="changes">{t("worktree.changes", { count: worktree.changes })}</span>
          )}
          {pr && (
            <button className="link" onClick={() => void api.openUrl(pr.url)}>
              <PrBadge summary={pr} />
            </button>
          )}
        </div>
        <div className="worktree-actions">
          <button
            className={`icon-button ${pinned ? "toggled" : ""}`}
            onClick={() => onTogglePin(path)}
            title={pinned ? t("sidebar.unpin") : t("sidebar.pin")}
            aria-label={pinned ? t("sidebar.unpin") : t("sidebar.pin")}
          >
            <PinIcon filled={pinned} />
          </button>
          {!pr && worktree.branch && !worktree.isMain && (
            <button onClick={() => addShell(CREATE_PR_COMMAND, t("worktree.createPrTab"))}>
              {t("worktree.createPr")}
            </button>
          )}
          <button onClick={() => void api.openInEditor(config.editorCommand, path).catch(showError)}>
            {t("worktree.openEditor")}
          </button>
          <button onClick={() => void api.revealInFinder(path).catch(showError)}>{t("worktree.reveal")}</button>
          <button className={showGit ? "toggled" : ""} onClick={() => setShowGit((v) => !v)}>
            {t("worktree.lazygit")}
          </button>
          <button onClick={() => onClose(path)} title={t("worktree.closeHint")}>
            {t("worktree.close")}
          </button>
          {!worktree.isMain && (
            <button className="danger" onClick={() => onRemove(worktree)}>
              {t("worktree.remove")}
            </button>
          )}
        </div>
      </header>

      <div className="worktree-body" ref={bodyRef}>
        <div className="pane" style={{ flexBasis: showGit ? `${split}%` : "100%" }}>
          <div className="tabs">
            {tabs.map((tab) => (
              <div
                key={tab.id}
                className={`tab ${tab.id === activeTab ? "active" : ""}`}
                onClick={() => setActiveTab(tab.id)}
              >
                <span>{tab.kind === "agent" ? t("worktree.agentTab", { command: tab.label }) : tab.label}</span>
                {tab.kind === "shell" && (
                  <button
                    className="tab-close"
                    aria-label={t("worktree.closeTab")}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeTab(tab.id);
                    }}
                  >
                    ×
                  </button>
                )}
              </div>
            ))}
            <button className="tab-add" onClick={() => addShell()} title={t("worktree.newShell")}>
              +
            </button>
          </div>
          <div className="pane-content">
            {tabs.map((tab) => (
              <div key={tab.id} className="terminal-slot" style={{ display: tab.id === activeTab ? "block" : "none" }}>
                <Terminal
                  id={tab.id}
                  cwd={path}
                  command={tab.command}
                  active={active && tab.id === activeTab}
                  onActivity={tab.kind === "agent" ? activity : undefined}
                  onBell={bell}
                />
              </div>
            ))}
          </div>
        </div>

        <div className="splitter" style={{ display: showGit ? "block" : "none" }} onMouseDown={startDrag} />

        <div className="pane git-pane" style={{ display: showGit ? "flex" : "none" }}>
          <div className="tabs">
            <div className="tab active static">{t("worktree.lazygit")}</div>
          </div>
          <div className="pane-content">
            <div className="terminal-slot">
              <Terminal
                id={sessionId.lazygit(path)}
                cwd={path}
                command={config.lazygitCommand || "lazygit"}
                active={false}
              />
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

export const WorktreeView = memo(WorktreeViewImpl);
