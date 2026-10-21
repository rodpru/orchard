/** Terminal session ids are built in one place; the backend treats them as opaque. */
export type SessionId = string & { readonly __brand: "SessionId" };

export const sessionId = {
  agent: (path: string) => `${path}::agent` as SessionId,
  shell: (path: string, n: number) => `${path}::shell::${n}` as SessionId,
  lazygit: (path: string) => `${path}::lazygit` as SessionId,
};

/** The shell number of `id` when it is a shell session of `path`. */
export function shellNumber(id: string, path: string): number | null {
  const match = id.startsWith(`${path}::shell::`) ? /^\d+$/.exec(id.slice(path.length + 9)) : null;
  return match ? Number(match[0]) : null;
}
