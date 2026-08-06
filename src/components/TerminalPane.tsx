import { useEffect, useRef } from "react";
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  toBytes,
  writeSession,
  resizeSession,
  spawnSession,
  type Bytes,
  type SessionType,
} from "../lib/ipc";
import { TERM_FONT } from "../lib/themes";
import { useAppearance } from "../lib/appearance";

// One xterm terminal bound to one backend session. Owns the terminal lifecycle,
// the output channel, keystroke write-back, container-driven resize, and live
// re-theming when the app appearance changes.
export function TerminalPane({
  sessionId,
  type,
  isolate,
  cwd,
  onExit,
}: {
  sessionId: string;
  type: SessionType;
  isolate?: boolean;
  cwd?: string;
  onExit: (id: string) => void;
}) {
  const elRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const { theme, appearance } = useAppearance();

  // Create the terminal once for this pane.
  useEffect(() => {
    const term = new Terminal({
      theme: theme.xterm,
      fontFamily: TERM_FONT,
      fontSize: appearance.fontSize,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(elRef.current!);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    // Clipboard: xterm would otherwise swallow Ctrl+V and send a literal ^V.
    // Returning false declines the event so the browser's native paste reaches
    // xterm's textarea. Ctrl+C copies when there's a selection, else falls
    // through as SIGINT (Windows Terminal behavior).
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const ctrl = e.ctrlKey && !e.altKey;
      if (ctrl && e.key.toLowerCase() === "v") return false;
      if (ctrl && e.key.toLowerCase() === "c") {
        const sel = term.getSelection();
        if (sel) {
          navigator.clipboard?.writeText(sel).catch(() => {});
          return false;
        }
      }
      return true;
    });

    term.onData((data) => {
      writeSession(sessionId, data).catch(() => {});
    });

    const channel = new Channel<Bytes>();
    channel.onmessage = (msg) => term.write(toBytes(msg));

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* element detached mid-teardown */
      }
      resizeSession(sessionId, term.rows, term.cols).catch(() => {});
    });
    ro.observe(elRef.current!);

    if (isolate) {
      term.write("\x1b[38;5;245m[mosaic] creating an isolated git worktree…\x1b[0m\r\n");
    }
    spawnSession(sessionId, channel, type.program, type.args, term.rows, term.cols, {
      isolate,
      cwd,
    }).catch((e) => term.write(`\r\n\x1b[31m[spawn error] ${e}\x1b[0m\r\n`));

    const unlisten = listen<string>("session-exited", (ev) => {
      if (ev.payload === sessionId) {
        term.write("\r\n\x1b[38;5;245m[session ended]\x1b[0m\r\n");
        onExit(sessionId);
      }
    });

    term.focus();

    return () => {
      ro.disconnect();
      unlisten.then((f) => f());
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // sessionId is stable for a pane's lifetime; theme/appearance handled below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Live re-theme + font-size on the already-open terminal.
  useEffect(() => {
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term || !fit) return;
    term.options.theme = theme.xterm;
    term.options.fontSize = appearance.fontSize;
    try {
      fit.fit();
    } catch {
      /* ignore */
    }
    resizeSession(sessionId, term.rows, term.cols).catch(() => {});
  }, [theme.id, theme.xterm, appearance.fontSize, sessionId]);

  return <div className="pane-term" ref={elRef} />;
}
