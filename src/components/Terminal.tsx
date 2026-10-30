import { Channel } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask } from "@tauri-apps/plugin-dialog";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { api, errorMessage, isOpenableUrl, type PtyMessage } from "../lib/api";
import { isMac } from "../lib/platform";
import type { SessionId } from "../lib/sessions";
import { droppedPaths } from "../lib/shell";

/** How long a resize must settle before the terminal refits. */
const FIT_DEBOUNCE_MS = 50;

const THEME = {
  background: "#16171d",
  foreground: "#d8dae3",
  cursor: "#e6e8ef",
  selectionBackground: "#3a3f55",
  scrollbarSliderBackground: "rgba(255, 255, 255, 0.06)",
  scrollbarSliderHoverBackground: "rgba(255, 255, 255, 0.12)",
  scrollbarSliderActiveBackground: "rgba(255, 255, 255, 0.18)",
  black: "#1d1f27",
  red: "#ef6b73",
  green: "#8bd49c",
  yellow: "#e8c46c",
  blue: "#6cb2f0",
  magenta: "#c49bf2",
  cyan: "#66d4d0",
  white: "#d8dae3",
  brightBlack: "#5c6170",
  brightRed: "#ff8a91",
  brightGreen: "#a6e5b4",
  brightYellow: "#f3d78f",
  brightBlue: "#8cc6f7",
  brightMagenta: "#d5b5f7",
  brightCyan: "#8ae3df",
  brightWhite: "#ffffff",
};

interface Props {
  id: SessionId;
  cwd: string;
  command: string | null;
  active: boolean;
  onActivity?: () => void;
  onBell?: () => void;
}

/**
 * A view of a backend session. Mounting attaches (starting the process if it
 * is not running) and replays recent output; unmounting only detaches, so the
 * process survives re-renders, StrictMode and webview reloads. Processes end
 * through `api.ptyKill*`, or here when `command` changes.
 */
export function Terminal({ id, cwd, command, active, onActivity, onBell }: Props) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const instanceRef = useRef<{ term: XTerm; fit: FitAddon } | null>(null);
  // Callbacks and translations change identity on every render; read them
  // through refs so the terminal is created only once.
  const latest = useRef({ onActivity, onBell, t });
  useEffect(() => {
    latest.current = { onActivity, onBell, t };
  });
  const attachedCommand = useRef(command);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // OSC 8 links show any text over any target, so the real target is
    // confirmed first. Plain URLs open directly: their text is the target.
    const openHyperlink = async (uri: string) => {
      if (!isOpenableUrl(uri)) return;
      const { t } = latest.current;
      // The parsed form spells an internationalised domain in punycode, so a
      // look-alike such as `аpple.com` (Cyrillic а) shows as `xn--pple-43d.com`.
      const confirmed = await ask(t("terminal.openLink", { url: new URL(uri).href }), {
        kind: "warning",
        okLabel: t("common.open"),
        cancelLabel: t("common.cancel"),
      });
      if (confirmed) await api.openUrl(uri);
    };

    const term = new XTerm({
      fontFamily: '"SF Mono", Menlo, Monaco, "DejaVu Sans Mono", "Liberation Mono", "Ubuntu Mono", monospace',
      fontSize: 13,
      lineHeight: 1.15,
      cursorBlink: true,
      scrollback: 10_000,
      macOptionIsMeta: true,
      allowProposedApi: true,
      theme: THEME,
      // xterm's default handler does nothing in this webview.
      linkHandler: { activate: (_event, uri) => void openHyperlink(uri).catch(() => undefined) },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((_event, uri) => void api.openUrl(uri).catch(() => undefined)));
    term.open(container);
    // The DOM renderer rebuilds every row a TUI redraws; WebGL draws them on
    // the GPU. WebKit keeps about 16 live WebGL contexts and drops the oldest,
    // so only terminals on screen hold one. Without WebGL2 (some WebKitGTK
    // builds) the DOM renderer stays.
    let webgl: WebglAddon | null = null;
    const visibility = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting && !webgl) {
        try {
          const addon = new WebglAddon();
          addon.onContextLoss(() => {
            addon.dispose();
            if (webgl === addon) webgl = null;
          });
          term.loadAddon(addon);
          webgl = addon;
        } catch {
          // DOM renderer
        }
      } else if (!entry.isIntersecting && webgl) {
        webgl.dispose();
        webgl = null;
      }
    });
    visibility.observe(container);
    if (container.offsetWidth > 0) fit.fit();
    instanceRef.current = { term, fit };

    let disposed = false;
    let exited = false;
    let token: number | null = null;
    let lastActivity = 0;

    const attach = () => {
      const channel = new Channel<PtyMessage>();
      // Output that arrives before the attach resolves is the replay of what
      // happened while detached; it is not new activity.
      let live = false;
      channel.onmessage = (message) => {
        if (disposed) return;
        if (message instanceof ArrayBuffer) {
          term.write(new Uint8Array(message));
          const now = Date.now();
          if (live && now - lastActivity > 1000) {
            lastActivity = now;
            latest.current.onActivity?.();
          }
        } else if (message.kind === "exit") {
          exited = true;
          term.write(`\r\n\x1b[2m${latest.current.t("terminal.exited")}\x1b[0m\r\n`);
        }
      };
      exited = false;
      api
        .ptyAttach({ id, cwd, command, cols: term.cols, rows: term.rows }, channel)
        .then((attached) => {
          if (disposed) void api.ptyDetach(id, attached);
          else {
            token = attached;
            live = true;
          }
        })
        .catch((error: unknown) => term.write(`\x1b[31m${errorMessage(error)}\x1b[0m\r\n`));
    };

    const restart = () => {
      term.reset();
      // Sync commands run in order, so the attach below starts a new process.
      void api.ptyKill(id);
      attach();
    };

    term.attachCustomKeyEventHandler((event) => {
      // Shift+Enter inserts a newline in agent prompts (Claude Code, Codex).
      if (event.type === "keydown" && event.key === "Enter" && event.shiftKey && !exited) {
        void api.ptyWrite(id, "\x1b\r");
        return false;
      }
      // Without a macOS Edit menu there is no ⌘C/⌘V, so copy and paste with
      // Ctrl+Shift+C/V like other Linux terminals; plain Ctrl+C stays SIGINT.
      if (!isMac && event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey) {
        if (event.code === "KeyC") {
          if (event.type === "keydown" && term.hasSelection()) {
            void api.writeClipboard(term.getSelection()).catch(() => undefined);
          }
          event.preventDefault();
          return false;
        }
        if (event.code === "KeyV") {
          if (event.type === "keydown" && !exited) {
            void api.readClipboard().then((text) => term.paste(text), () => undefined);
          }
          event.preventDefault();
          return false;
        }
      }
      return true;
    });
    term.onData((data) => {
      if (exited) {
        if (data === "\r") restart();
        return;
      }
      api.ptyWrite(id, data).catch(() => undefined);
    });
    term.onResize(({ cols, rows }) => {
      if (!exited) api.ptyResize(id, cols, rows).catch(() => undefined);
    });
    term.onBell(() => latest.current.onBell?.());

    // Tauri takes file drops away from the page; paste the paths of files
    // dropped on this terminal, as Terminal.app does. Agents such as Claude
    // Code attach a pasted image path as an image.
    const stopDrop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (disposed || exited || payload.type !== "drop" || payload.paths.length === 0) return;
      const rect = container.getBoundingClientRect();
      const x = payload.position.x / window.devicePixelRatio;
      const y = payload.position.y / window.devicePixelRatio;
      if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) return;
      term.paste(droppedPaths(payload.paths));
      term.focus();
    });

    // Dragging the splitter or the window edge resizes on every frame; each
    // fit reflows xterm and resizes the PTY, so only the last size counts.
    // Mounting and switching tabs fit right away above and below.
    let fitTimer: number | undefined;
    const observer = new ResizeObserver(() => {
      window.clearTimeout(fitTimer);
      fitTimer = window.setTimeout(() => {
        if (container.offsetWidth > 0 && container.offsetHeight > 0) fit.fit();
      }, FIT_DEBOUNCE_MS);
    });
    observer.observe(container);

    if (attachedCommand.current !== command) {
      // The configured command changed (e.g. lazygit in Settings): replace the process.
      attachedCommand.current = command;
      restart();
    } else {
      attach();
    }

    return () => {
      disposed = true;
      void stopDrop.then((stop) => stop());
      observer.disconnect();
      window.clearTimeout(fitTimer);
      visibility.disconnect();
      if (token !== null) void api.ptyDetach(id, token).catch(() => undefined);
      term.dispose();
      instanceRef.current = null;
    };
  }, [id, cwd, command]);

  useEffect(() => {
    const instance = instanceRef.current;
    if (!active || !instance) return;
    const frame = requestAnimationFrame(() => {
      instance.fit.fit();
      instance.term.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [active]);

  return <div className="terminal" ref={containerRef} />;
}
