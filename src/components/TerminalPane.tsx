import { useEffect, useRef, useState } from "react";
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
  type SavedWorktree,
  type SessionType,
} from "../lib/ipc";
import { TERM_FONT } from "../lib/themes";
import { useAppearance } from "../lib/appearance";

// `spawn_session` rejects with a structured SpawnError, not a string, so that a
// refusal can say *why*. Interpolating that object directly would print
// "[object Object]" — swallowing the reason precisely when the user most needs
// it, since the commonest refusal is "you asked for isolation and could not
// have it". Non-Tauri rejections (a thrown Error, a plain string) still arrive
// here, so fall back rather than assume the shape.
function spawnErrorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    const err = e as { message?: unknown; isolation?: { reason?: unknown } };
    const message = typeof err.message === "string" ? err.message : String(e);
    // The reason is what tells the user whether to retry, pick a git repo, or
    // deliberately continue without isolation.
    const reason = err.isolation?.reason;
    return typeof reason === "string" ? `${message} (${reason})` : message;
  }
  return String(e);
}

function isIsolationUnavailable(e: unknown): boolean {
  return Boolean(e && typeof e === "object" && "kind" in e && e.kind === "isolationUnavailable");
}

// One xterm terminal bound to one backend session. Owns the terminal lifecycle,
// the output channel, keystroke write-back, container-driven resize, and live
// re-theming when the app appearance changes.
export function TerminalPane({
  sessionId,
  type,
  isolate,
  cwd,
  reuseWorktree,
  onExit,
  onIsolationChange,
  onSpawnError,
}: {
  sessionId: string;
  type: SessionType;
  isolate?: boolean;
  cwd?: string;
  // The worktree this session ran in before the app was last closed. Passed
  // through so a restored isolated pane rejoins its own worktree rather than
  // abandoning it and cutting a second one.
  reuseWorktree?: SavedWorktree;
  onExit: (id: string) => void;
  // Isolation was refused and the user chose to carry on without it, so the
  // pane's remembered isolation has to change with it.
  onIsolationChange: (id: string, isolate: boolean) => void;
  // A spawn that never started. The pane shows the reason itself; this lets the
  // app say so somewhere the user is looking, which matters most on startup
  // when several panes come back at once.
  onSpawnError?: (id: string, message: string) => void;
}) {
  const elRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const fitFrameRef = useRef<number | null>(null);
  const scheduleFitRef = useRef<() => void>(() => {});
  const lastSizeRef = useRef({ rows: 0, cols: 0 });
  const { theme, appearance } = useAppearance();
  const [isolationError, setIsolationError] = useState<string | null>(null);
  const continueWithoutIsolationRef = useRef<() => void>(() => {});

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
    const start = (withIsolation: boolean) => {
      setIsolationError(null);
      spawnSession(sessionId, channel, type.program, type.args, term.rows, term.cols, {
        isolate: withIsolation,
        cwd,
        // Only an isolated attempt may rejoin the saved worktree. Carrying on
        // without isolation means there is no worktree to rejoin, and handing
        // one over anyway would ask the backend for a contradiction.
        reuseWorktree: withIsolation ? reuseWorktree : undefined,
      }).catch((e) => {
        const message = spawnErrorText(e);
        writeAfterQueuedOutput(`\r\n\x1b[31m[spawn error] ${message}\x1b[0m\r\n`);
        if (isIsolationUnavailable(e)) setIsolationError(message);
        onSpawnError?.(sessionId, message);
      });
    };
    continueWithoutIsolationRef.current = () => {
      onIsolationChange(sessionId, false);
      start(false);
    };
    start(Boolean(isolate));

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
      continueWithoutIsolationRef.current = () => {};
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

  return (
    <div className="pane-term-wrap">
      {isolationError && (
        <div className="pane-spawn-error" role="alert">
          <span>{isolationError}</span>
          <button type="button" onClick={() => continueWithoutIsolationRef.current()}>
            Continue without isolation
          </button>
        </div>
      )}
      <div className="pane-term" ref={elRef} />
    </div>
  );
}
