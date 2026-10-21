# Orchard

Run coding agents in parallel git worktrees, on macOS.

Each task gets its own worktree, and each worktree gets:

- an agent terminal running Claude Code, Codex, opencode or any other CLI;
- extra shells, as tabs;
- a lazygit pane next to it;
- the PR and CI status of its branch, from the `gh` CLI.

The sidebar shows which agents are busy, which have new output and which rang the bell. Everything runs locally. Orchard is a desktop app built with [Tauri](https://tauri.app), Rust and React.

## Requirements

- macOS
- [`lazygit`](https://github.com/jesseduffield/lazygit) and [`gh`](https://cli.github.com) on your shell PATH, with `gh auth login` done
- The agent CLI you want to use, for example `claude`

To build it you also need Rust (`brew install rustup && rustup default stable`), Node and pnpm.

## Install

There are no prebuilt releases yet. Build it from source:

```bash
git clone https://github.com/rodpru/orchard.git
cd orchard
pnpm install
pnpm tauri build
cp -R src-tauri/target/release/bundle/macos/Orchard.app /Applications/
```

The build also produces a `.dmg` in `src-tauri/target/release/bundle/dmg/`.

## Use

1. Click **Add repository** and pick a git repository.
2. Press `⌘N` to create a worktree. Give it a branch name and, if the branch is new, the base to start from.
3. The agent starts in the new worktree. Open more shells with `+`, and push and open a PR with **Create PR**.
4. When the task is done, **Remove** deletes the worktree. Orchard asks whether to also delete the local branch; it never deletes the remote branch.

### Open a worktree from the terminal

```bash
ln -s /Applications/Orchard.app/Contents/MacOS/orchard /opt/homebrew/bin/orchard
orchard .
```

`orchard <path>` registers the repository that contains `<path>` and opens its worktree.

### Shortcuts

- `⌘N` new worktree in the current repository
- `⌘1`…`⌘9` switch between open worktrees
- `Shift+Enter` newline in agent prompts

## Where things live

- **Config:** `~/Library/Application Support/io.github.rodpru.orchard/config.json`. If it cannot be parsed, it is moved to `config.json.corrupt-<timestamp>` and Orchard starts with defaults.
- **Terminals:** owned by the app, not the window. Closing a tab or a worktree view stops its processes; reloading the window does not. Quitting Orchard stops all of them.
- **New worktrees:** `<repo>.worktrees/<branch>` next to the repository, or `<folder>/<repo>/<branch>` when a worktree folder is set in Settings.

## Develop

```bash
pnpm install
pnpm tauri dev
```

Checks, also run in CI:

```bash
pnpm exec tsc --noEmit && pnpm lint && pnpm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

IPC types in `src/lib/bindings/` are generated from the Rust structs. After changing one, run `pnpm gen:types` and commit the result.

## Security

Orchard runs the commands you configure, by design. See [SECURITY.md](SECURITY.md) for details and for how to report a vulnerability.

## License

[MIT](LICENSE)
