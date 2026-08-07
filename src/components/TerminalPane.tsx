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
  const fitFrameRef = useRef<number | null>(null);
  const scheduleFitRef = useRef<() => void>(() => {});
  const lastSizeRef = useRef({ rows: 0, cols: 0 });
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

    let resizeInFlight = false;
    let desiredSize: { rows: number; cols: number } | null = null;
    let alive = true;
    const pumpResize = async () => {
      if (resizeInFlight) return;
      resizeInFlight = true;
      while (alive && desiredSize) {
        const next = desiredSize;
        desiredSize = null;
        await resizeSession(sessionId, next.rows, next.cols).catch(() => {});
      }
      resizeInFlight = false;
    };

    const fitAndResize = () => {
      fitFrameRef.current = null;
      const el = elRef.current;
      // Focus mode temporarily hides the other panes. Do not collapse their
      // PTYs to zero columns; the observer runs again when they reappear.
      if (!el || el.clientWidth < 40 || el.clientHeight < 40) return;
      try {
        fit.fit();
      } catch {
        return;
      }
      const last = lastSizeRef.current;
      if (term.rows === last.rows && term.cols === last.cols) return;
      lastSizeRef.current = { rows: term.rows, cols: term.cols };
      desiredSize = { rows: term.rows, cols: term.cols };
      void pumpResize();
    };
    const scheduleFit = () => {
      if (fitFrameRef.current !== null) cancelAnimationFrame(fitFrameRef.current);
      fitFrameRef.current = requestAnimationFrame(fitAndResize);
    };
    scheduleFitRef.current = scheduleFit;
    lastSizeRef.current = { rows: term.rows, cols: term.cols };

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
    const outputQueue: Uint8Array[] = [];
    let outputBytes = 0;
    let outputFrame: number | null = null;
    const flushOutput = () => {
      outputFrame = null;
      if (outputQueue.length === 0) return;
      outputBytes = 0;
      if (outputQueue.length === 1) {
        term.write(outputQueue.shift()!);
        return;
      }
      const total = outputQueue.reduce((n, chunk) => n + chunk.byteLength, 0);
      const merged = new Uint8Array(total);
      let offset = 0;
      for (const chunk of outputQueue.splice(0)) {
        merged.set(chunk, offset);
        offset += chunk.byteLength;
      }
      term.write(merged);
    };
    channel.onmessage = (msg) => {
      const bytes = toBytes(msg);
      outputQueue.push(bytes);
      outputBytes += bytes.byteLength;
      if (outputBytes >= 64 * 1024) {
        if (outputFrame !== null) cancelAnimationFrame(outputFrame);
        flushOutput();
      } else if (outputFrame === null) {
        outputFrame = requestAnimationFrame(flushOutput);
      }
    };
    const writeAfterQueuedOutput = (data: string) => {
      if (outputFrame !== null) cancelAnimationFrame(outputFrame);
      flushOutput();
      term.write(data);
    };

    const ro = new ResizeObserver(scheduleFit);
    ro.observe(elRef.current!);

    if (isolate) {
      term.write("\x1b[38;5;245m[mosaic] creating an isolated git worktree…\x1b[0m\r\n");
    }
    spawnSession(sessionId, channel, type.program, type.args, term.rows, term.cols, {
      isolate,
      cwd,
    }).catch((e) => writeAfterQueuedOutput(`\r\n\x1b[31m[spawn error] ${e}\x1b[0m\r\n`));

    const unlisten = listen<string>("session-exited", (ev) => {
      if (ev.payload === sessionId) {
        writeAfterQueuedOutput("\r\n\x1b[38;5;245m[session ended]\x1b[0m\r\n");
        onExit(sessionId);
      }
    });

    term.focus();

    return () => {
      alive = false;
      desiredSize = null;
      ro.disconnect();
      if (fitFrameRef.current !== null) cancelAnimationFrame(fitFrameRef.current);
      if (outputFrame !== null) cancelAnimationFrame(outputFrame);
      scheduleFitRef.current = () => {};
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
    scheduleFitRef.current();
  }, [theme.id, theme.xterm, appearance.fontSize, sessionId]);

  return <div className="pane-term" ref={elRef} />;
}
