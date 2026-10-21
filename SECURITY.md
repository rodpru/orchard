# Security

## What Orchard runs

Orchard is a local developer tool. By design it starts programs on your machine:

- the agent, lazygit and editor commands set in Settings, through your login shell;
- `git` and `gh` in the repositories you add;
- any command you type in its terminals.

Treat the Settings commands like shell aliases: anyone who can edit `config.json` can make Orchard run commands as you. Orchard makes no network requests of its own; `gh` talks to GitHub with your existing login.

The webview runs under a strict Content Security Policy and can only open `https://` links, and `http://` links to `localhost`, `127.0.0.1` or `[::1]`.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private vulnerability reporting instead: the **Security** tab of this repository, then **Report a vulnerability**. You should get a reply within a week.
