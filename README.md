# Orchard

Run coding agents in parallel git worktrees. Each worktree gets an agent terminal (Claude Code, Codex, opencode or any CLI), extra shells, and a lazygit pane that opens with it. PR and CI status come from the `gh` CLI. Everything runs locally.

## Requirements

- macOS, Rust (`brew install rustup && rustup default stable`), Node + pnpm
- `lazygit` and `gh` (logged in) on your shell PATH

## Develop

```bash
pnpm install
pnpm tauri dev
```

Checks (also run in CI):

```bash
pnpm exec tsc --noEmit && pnpm lint && pnpm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

IPC types in `src/lib/bindings/` are generated from the Rust structs. After changing one, run `pnpm gen:types` and commit the result.

## Build

```bash
pnpm tauri build
```

Output: `src-tauri/target/release/bundle/macos/Orchard.app` and a `.dmg`.

## Open a worktree from the terminal

```bash
ln -s /Applications/Orchard.app/Contents/MacOS/orchard /opt/homebrew/bin/orchard
orchard .
```

`orchard <path>` registers the repository that contains `<path>` and opens its worktree.

## Shortcuts

- `⌘N` new worktree in the current repository
- `⌘1`…`⌘9` switch between open worktrees
- `Shift+Enter` newline in agent prompts

## Where things live

- Config: `~/Library/Application Support/dev.orchard.app/config.json`. If it cannot be parsed, it is moved to `config.json.corrupt-<timestamp>` next to it and Orchard starts with defaults.
- Terminals: owned by the app, not the window. Closing a worktree view's tab or the view itself stops its processes; reloading the window does not. Quitting Orchard stops all of them.
- New worktrees: `<repo>.worktrees/<branch>` next to the repository, or `<root>/<repo>/<branch>` when a worktree folder is set in Settings.
