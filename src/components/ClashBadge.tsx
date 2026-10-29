import { useTranslation } from "react-i18next";
import { basename } from "../lib/api";
import type { Clash, ClashKind, ClashTarget } from "../lib/types";

/** Most files named per line of the tooltip. */
const MAX_FILES = 3;
/** Most lines in the tooltip; the worst clashes come first. */
const MAX_LINES = 8;
const RANK: Record<ClashKind, number> = { conflict: 2, diverged: 1, overlap: 0 };
const ICON: Record<ClashKind, string> = { conflict: "⚠", diverged: "⇅", overlap: "≈" };

export function ClashBadge({ clashes }: { clashes: Clash[] }) {
  const { t } = useTranslation();
  const sorted = [...clashes].sort((a, b) => RANK[b.kind] - RANK[a.kind]);
  const worst = sorted[0]?.kind;
  if (!worst) return null;

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

  return (
    <span className={`clash-badge clash-${worst}`} title={lines.join("\n")} aria-label={lines.join("\n")}>
      {ICON[worst]}
      {clashes.length > 1 && <span className="clash-count">{clashes.length}</span>}
    </span>
  );
}
