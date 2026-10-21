# Orchard

Run coding agents in parallel git worktrees, on macOS and Linux.

![Orchard with Claude Code in the agent terminal and lazygit beside it](docs/screenshot.png)

Each task gets its own worktree, and each worktree gets:

- an agent terminal running Claude Code, Codex, opencode or any other CLI;
- extra shells, as tabs;
- a lazygit pane next to it;
- the PR and CI status of its branch, from the `gh` CLI.

The sidebar shows which agents are busy, which have new output and which rang the bell. Everything runs locally. Orchard is a desktop app built with [Tauri](https://tauri.app), Rust and React.

## Requirements

- macOS, or Linux with WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+, Fedora 36+)
- [`lazygit`](https://github.com/jesseduffield/lazygit) and [`gh`](https://cli.github.com) on your shell PATH, with `gh auth login` done
- The agent CLI you want to use, for example `claude`

To build it you also need Rust (`brew install rustup && rustup default stable`), Node and pnpm. On Linux, also the WebKitGTK headers; on Debian or Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libsoup-3.0-dev
```

## Install

### macOS

```bash
brew install --cask rodpru/tap/orchard
```

Orchard is not notarized by Apple. The cask removes the quarantine flag after install, so macOS opens the app without a Gatekeeper prompt. Install it only if you trust the [tap](https://github.com/rodpru/homebrew-tap).

### Linux

Download the `.deb` from the [latest release](https://github.com/rodpru/orchard/releases/latest) and install it:

```bash
sudo apt install ./Orchard_*_amd64.deb
```

### Build from source

```bash
git clone https://github.com/rodpru/orchard.git
cd orchard
pnpm install
pnpm tauri build
```

On macOS, copy the app with `cp -R src-tauri/target/release/bundle/macos/Orchard.app /Applications/`; the build also produces a `.dmg` in `src-tauri/target/release/bundle/dmg/`. On Linux, install the `.deb` from `src-tauri/target/release/bundle/deb/`.

## Use

1. Click **Add repository** and pick a git repository.
2. Press `⌘N` (Linux: `Ctrl+Shift+N`) to create a worktree. Give it a branch name and, if the branch is new, the base to start from.
3. The agent starts in the new worktree. Open more shells with `+`, and push and open a PR with **Create PR**.
4. When the task is done, **Remove** deletes the worktree. Orchard asks whether to also delete the local branch; it never deletes the remote branch.

### Open a worktree from the terminal

The Homebrew cask and the `.deb` put `orchard` on your PATH. If you built the macOS app from source, link it yourself:

```bash
ln -s /Applications/Orchard.app/Contents/MacOS/orchard /opt/homebrew/bin/orchard
```

Then:

```bash
orchard .
```

`orchard <path>` registers the repository that contains `<path>` and opens its worktree.

### Shortcuts

| | macOS | Linux |
| --- | --- | --- |
| New worktree in the current repository | `⌘N` | `Ctrl+Shift+N` |
| Switch between open worktrees | `⌘1`…`⌘9` | `Ctrl+Shift+1`…`9` |
| Copy and paste in a terminal | `⌘C`, `⌘V` | `Ctrl+Shift+C`, `Ctrl+Shift+V` |
| Newline in agent prompts | `Shift+Enter` | `Shift+Enter` |

## Where things live

- **Config:** `~/Library/Application Support/io.github.rodpru.orchard/config.json` on macOS, `~/.config/io.github.rodpru.orchard/config.json` on Linux. If it cannot be parsed, it is moved to `config.json.corrupt-<timestamp>` and Orchard starts with defaults.
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
