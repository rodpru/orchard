/** Characters a shell would split on or interpret, escaped like Terminal.app does for dropped files. */
const SPECIAL = /[\s!"#$&'()*,;<>?[\\\]^`{|}~]/g;

export function shellEscape(path: string): string {
  return path.replace(SPECIAL, (c) => `\\${c}`);
}

/** Dropped file paths as terminal input: escaped, space-separated, with a trailing space. */
export function droppedPaths(paths: string[]): string {
  return `${paths.map(shellEscape).join(" ")} `;
}
