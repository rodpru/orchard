import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { LANGUAGES } from "../i18n";
import { api, basename, errorMessage } from "../lib/api";
import type { Branches, Config, Language, Worktree } from "../lib/types";

function Modal({ title, children, onClose }: { title: string; children: ReactNode; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={title}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

interface NewWorktreeProps {
  repo: string;
  config: Config;
  onCancel: () => void;
  onCreated: (path: string, agentCommand: string) => Promise<void>;
}

export function NewWorktreeDialog({ repo, config, onCancel, onCreated }: NewWorktreeProps) {
  const { t } = useTranslation();
  const [branches, setBranches] = useState<Branches | null>(null);
  const [branch, setBranch] = useState("");
  const [base, setBase] = useState("");
  const [agent, setAgent] = useState(config.agentCommand);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listBranches(repo)
      .then((result) => {
        setBranches(result);
        setBase(result.defaultBranch);
      })
      .catch((e: unknown) => setError(errorMessage(e)));
  }, [repo]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const path = await api.createWorktree({
        repo,
        branch: branch.trim(),
        base: base || null,
        root: config.worktreeRoot || null,
      });
      await onCreated(path, agent.trim());
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title={t("newWorktree.title", { repo: basename(repo) })} onClose={onCancel}>
      <form onSubmit={(e) => void submit(e)}>
        <label>
          {t("newWorktree.branch")}
          <input
            autoFocus
            value={branch}
            onChange={(e) => setBranch(e.target.value)}
            placeholder={t("newWorktree.branchPlaceholder")}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <label>
          {t("newWorktree.base")}
          <select value={base} onChange={(e) => setBase(e.target.value)} disabled={!branches}>
            {branches && (
              <>
                <optgroup label={t("newWorktree.local")}>
                  {branches.local.map((b) => (
                    <option key={b} value={b}>
                      {b}
                    </option>
                  ))}
                </optgroup>
                <optgroup label={t("newWorktree.remote")}>
                  {branches.remote.map((b) => (
                    <option key={b} value={b}>
                      {b}
                    </option>
                  ))}
                </optgroup>
              </>
            )}
          </select>
          <small>{t("newWorktree.baseHint")}</small>
        </label>
        <label>
          {t("newWorktree.agent")}
          <input value={agent} onChange={(e) => setAgent(e.target.value)} spellCheck={false} />
          <small>{t("newWorktree.agentHint")}</small>
        </label>
        {error && <p className="error">{error}</p>}
        <div className="modal-actions">
          <button type="button" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy || !branch.trim()}>
            {busy ? t("newWorktree.creating") : t("newWorktree.create")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

interface RemoveProps {
  worktree: Worktree;
  onCancel: () => void;
  onConfirm: (options: { force: boolean; deleteBranch: boolean }) => Promise<void>;
}

export function RemoveWorktreeDialog({ worktree, onCancel, onConfirm }: RemoveProps) {
  const { t } = useTranslation();
  const [force, setForce] = useState(false);
  const [deleteBranch, setDeleteBranch] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dirty = worktree.changes > 0;

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onConfirm({ force, deleteBranch });
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title={t("remove.title")} onClose={onCancel}>
      <p>{t("remove.body", { branch: worktree.branch ?? basename(worktree.path), path: worktree.path })}</p>
      {dirty && <p className="warning">{t("remove.dirty", { count: worktree.changes })}</p>}
      <label className="checkbox">
        <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} />
        {t("remove.force")}
      </label>
      {worktree.branch && (
        <label className="checkbox">
          <input type="checkbox" checked={deleteBranch} onChange={(e) => setDeleteBranch(e.target.checked)} />
          {t("remove.deleteBranch", { branch: worktree.branch })}
        </label>
      )}
      {error && <p className="error">{error}</p>}
      <div className="modal-actions">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="danger solid" disabled={busy || (dirty && !force)} onClick={() => void submit()}>
          {t("remove.confirm")}
        </button>
      </div>
    </Modal>
  );
}

interface SettingsProps {
  config: Config;
  /** Why the last update check failed, if it did. */
  updateError: string | null;
  onCancel: () => void;
  onSave: (config: Config) => void;
}

export function SettingsDialog({ config, updateError, onCancel, onSave }: SettingsProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(config);
  const field = (key: "agentCommand" | "lazygitCommand" | "editorCommand" | "worktreeRoot") => ({
    value: draft[key],
    spellCheck: false,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setDraft({ ...draft, [key]: e.target.value }),
  });

  return (
    <Modal title={t("settings.title")} onClose={onCancel}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          onSave(draft);
        }}
      >
        <label>
          {t("settings.agentCommand")}
          <input {...field("agentCommand")} />
          <small>{t("settings.agentCommandHint")}</small>
        </label>
        <label>
          {t("settings.lazygitCommand")}
          <input {...field("lazygitCommand")} />
        </label>
        <label>
          {t("settings.editorCommand")}
          <input {...field("editorCommand")} />
          <small>{t("settings.editorCommandHint")}</small>
        </label>
        <label>
          {t("settings.worktreeRoot")}
          <input {...field("worktreeRoot")} placeholder="~/worktrees" />
          <small>{t("settings.worktreeRootHint")}</small>
        </label>
        <label>
          {t("settings.language")}
          <select
            value={draft.language ?? ""}
            onChange={(e) => setDraft({ ...draft, language: (e.target.value || null) as Language | null })}
          >
            <option value="">{t("settings.languageAuto")}</option>
            {LANGUAGES.map((lang) => (
              <option key={lang} value={lang}>
                {t(`settings.languages.${lang}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={draft.autoUpdate}
            onChange={(e) => setDraft({ ...draft, autoUpdate: e.target.checked })}
          />
          {t("settings.autoUpdate")}
        </label>
        <small>{t("settings.autoUpdateHint")}</small>
        {updateError && <p className="error small">{t("settings.updateError", { error: updateError })}</p>}
        <label className="checkbox">
          <input
            type="checkbox"
            checked={draft.fetchRemote}
            onChange={(e) => setDraft({ ...draft, fetchRemote: e.target.checked })}
          />
          {t("settings.fetchRemote")}
        </label>
        <small>{t("settings.fetchRemoteHint")}</small>
        <div className="modal-actions">
          <button type="button" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary">
            {t("common.save")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
