import { useId } from "react";
import { useTranslation } from "react-i18next";
import { basename } from "../lib/api";
import type { Clash, ClashKind, ClashTarget } from "../lib/types";

/** Most files named per line of the tooltip. */
const MAX_FILES = 3;
/** Most lines in the tooltip; the worst clashes come first. */
const MAX_LINES = 8;
const RANK: Record<ClashKind, number> = { conflict: 2, diverged: 1, overlap: 0 };

/** Local clashes are with other worktrees; online ones with `origin` and open PRs. */
type Scope = "local" | "online";

function TreeIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="8" cy="5" r="4" fill="currentColor" />
      <circle cx="4.6" cy="7.6" r="3.2" fill="currentColor" />
      <circle cx="11.4" cy="7.6" r="3.2" fill="currentColor" />
      <rect x="7" y="9" width="2" height="6" rx="0.6" fill="currentColor" />
    </svg>
  );
}

/** The git diamond with the branch cut out, so it reads on any background. */
function GitIcon() {
  // React ids carry characters like `:` or `«` that are unsafe inside `url(#…)`.
  const mask = `clash-git-${useId().replace(/[^\w-]/g, "")}`;
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <mask id={mask}>
        <rect width="16" height="16" fill="white" />
        <path d="M6.4 5.2v5.4M6.4 6.2l3 2.6" stroke="black" strokeWidth="1.3" strokeLinecap="round" fill="none" />
        <circle cx="6.4" cy="5.2" r="1.15" fill="black" />
        <circle cx="6.4" cy="10.7" r="1.15" fill="black" />
        <circle cx="9.7" cy="9.1" r="1.15" fill="black" />
      </mask>
      <rect x="2.7" y="2.7" width="10.6" height="10.6" rx="1.8" transform="rotate(45 8 8)" fill="currentColor" mask={`url(#${mask})`} />
    </svg>
  );
}

export function ClashBadge({ clashes }: { clashes: Clash[] }) {
  const local = clashes.filter((clash) => clash.target.type === "worktree");
  const online = clashes.filter((clash) => clash.target.type !== "worktree");
  return (
    <>
      {local.length > 0 && <ScopeBadge scope="local" clashes={local} />}
      {online.length > 0 && <ScopeBadge scope="online" clashes={online} />}
    </>
  );
}

function ScopeBadge({ scope, clashes }: { scope: Scope; clashes: Clash[] }) {
  const { t } = useTranslation();
  const sorted = [...clashes].sort((a, b) => RANK[b.kind] - RANK[a.kind]);
  const worst = sorted[0].kind;
  const conflicts = clashes.filter((clash) => clash.kind === "conflict").length;

  const target = (target: ClashTarget) => {
    switch (target.type) {
      case "worktree":
        return target.branch ?? basename(target.path);
      case "pr":
        return t("conflicts.pr", { number: target.number });
      default:
        return target.name;
    }
  };
  const files = (list: string[]) =>
    list.length <= MAX_FILES
      ? list.join(", ")
      : `${list.slice(0, MAX_FILES).join(", ")} ${t("conflicts.more", { count: list.length - MAX_FILES })}`;
  const lines = sorted.slice(0, MAX_LINES).map(({ kind, target: to, files: list }) =>
    t(kind === "diverged" && list.length === 0 ? "conflicts.divergedClean" : `conflicts.${kind}`, {
      target: target(to),
      files: files(list),
    }),
  );
  if (sorted.length > MAX_LINES) lines.push(t("conflicts.more", { count: sorted.length - MAX_LINES }));
  const label = [t(`conflicts.${scope}`), ...lines].join("\n");

  return (
    <span className={`clash-badge clash-${worst}`} title={label} aria-label={label}>
      {scope === "local" ? <TreeIcon /> : <GitIcon />}
      {conflicts > 0 && <span className="clash-count">{conflicts}</span>}
    </span>
  );
}
