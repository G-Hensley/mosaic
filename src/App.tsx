import { useEffect, useMemo, useRef, useState, type CSSProperties, type DragEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { TerminalPane } from "./components/TerminalPane";
import { SessionLauncher } from "./components/SessionLauncher";
import { SettingsPanel } from "./components/SettingsPanel";
import { ContextSidebar } from "./components/ContextSidebar";
import { ConductorBar } from "./components/ConductorBar";
import { DispatchDialog } from "./components/DispatchDialog";
import { ToastContainer } from "./components/Toast";
import {
  LayoutPanel,
  type LayoutColumns,
  type LayoutMode,
} from "./components/LayoutPanel";
import {
  killSession,
  setAgentBrain,
  conductorState,
  setConductor,
  haltConductor,
  setProject as setProjectDir,
  dispatchTask,
  type SavedWorktree,
  type SessionType,
  type SessionWorktreeEvent,
} from "./lib/ipc";
import { loadRoster, saveRoster, seedCounter } from "./lib/panes";
import { brainColorMap } from "./lib/brains";
import "./App.css";

type PaneStatus = "running" | "exited";
type Pane = {
  id: string;
  type: SessionType;
  status: PaneStatus;
  brain: string;
  isolate: boolean;
  // Where an isolated pane's worktree is, once the backend reports it. Persisted
  // so the pane can be put back into that same worktree after a restart.
  worktree?: SavedWorktree;
};

function App() {
  // The panes that were open when the app was last closed. Read once, before the
  // first render, so the CLIs come back with the window instead of after it.
  const [roster] = useState(loadRoster);
  // A restored pane is spawned fresh, so it starts "running" whatever it was
  // doing last time; the spawn corrects it if the CLI never comes up.
  const [panes, setPanes] = useState<Pane[]>(() =>
    roster.panes.map((p) => ({ ...p, status: "running" as const })),
  );
  // Restores that did not work out — a session type that no longer exists, a CLI
  // that would not start, a worktree that cannot be reused. Shown once, together,
  // so one bad pane is a message rather than a silently missing agent.
  const [restoreProblems, setRestoreProblems] = useState<string[]>(() => roster.problems);
  const restoredIds = useRef(new Set(roster.panes.map((p) => p.id)));
  const [launcherOpen, setLauncherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [layoutOpen, setLayoutOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [layout, setLayout] = useState<LayoutMode>(() => {
    try {
      return localStorage.getItem("mosaic.layout") === "fit" ? "fit" : "scroll";
    } catch {
      return "scroll";
    }
  });
  const [focusedPane, setFocusedPane] = useState<string | null>(null);
  const [layoutColumns, setLayoutColumns] = useState<LayoutColumns>(() => {
    try {
      const value = localStorage.getItem("mosaic.layout.columns");
      return /^[1-6]$/.test(value ?? "")
        ? (Number(value) as LayoutColumns)
        : "auto";
    } catch {
      return "auto";
    }
  });
  const [paneHeight, setPaneHeight] = useState(() => {
    try {
      const value = Number(localStorage.getItem("mosaic.layout.paneHeight"));
      return value >= 280 && value <= 720 ? value : 420;
    } catch {
      return 420;
    }
  });
  const [selectedBrain, setSelectedBrain] = useState("main");
  // The git repo sessions run in. Isolated sessions get worktrees of it.
  const [project, setProject] = useState<string | null>(() => {
    try {
      return localStorage.getItem("mosaic.project");
    } catch {
      return null;
    }
  });
  // Which pane (if any) is the conductor. Owned by the app, never self-claimed.
  const [conductor, setConductorName] = useState<string | null>(null);
  const [conductorTasks, setConductorTasks] = useState<import("./lib/ipc").ConductorTask[]>([]);
  const [conductorHalted, setConductorHalted] = useState(false);
  const [dispatchOpen, setDispatchOpen] = useState(false);
  const [toasts, setToasts] = useState<string[]>([]); // dismissed task ids
  // Past the highest restored id: restored panes keep the ids they had, and a
  // counter starting from zero would hand a new session an id that is already
  // live, which the backend refuses.
  const counter = useRef(seedCounter(roster.panes.map((p) => p.id)));
  const dragId = useRef<string | null>(null);

  // Put each restored pane's agent back in the brain it belonged to. The panes
  // themselves are already in state (and spawning); this is the half of
  // `addSession` that lives outside the pane list.
  useEffect(() => {
    for (const pane of roster.panes) {
      setAgentBrain(pane.id, pane.brain).catch(() => {});
    }
    // Runs once, against the roster captured before the first render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Remember the roster on every change, so a restart brings back whatever was
  // open at the moment it happened. Status is deliberately not stored: a
  // restored pane is a new process, so its state is whatever the new spawn
  // reports.
  useEffect(() => {
    saveRoster(
      panes.map((p) => ({
        id: p.id,
        typeId: p.type.id,
        brain: p.brain,
        isolate: p.isolate,
        worktree: p.worktree,
      })),
    );
  }, [panes]);

  // The backend reports which worktree an isolated session actually got, once it
  // is live. Recording it is what lets the next launch return the pane to that
  // worktree instead of stranding it — an abandoned worktree can hold
  // uncommitted agent work that nothing in the app would point at any more.
  useEffect(() => {
    const unlisten = listen<SessionWorktreeEvent>("session-worktree", (ev) => {
      const { sessionId, ...worktree } = ev.payload;
      setPanes((p) => p.map((x) => (x.id === sessionId ? { ...x, worktree } : x)));
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // A pane that never started. Only reported for restored panes: a session the
  // user just launched by hand has their attention already, and the failure is
  // printed in its terminal either way.
  function noteSpawnFailure(id: string, message: string) {
    if (!restoredIds.current.has(id)) return;
    setRestoreProblems((problems) => {
      const note = `Could not restore ${id}: ${message}`;
      return problems.includes(note) ? problems : [...problems, note];
    });
  }

  useEffect(() => {
    let alive = true;
    const refresh = async () => {
      try {
        const s = await conductorState();
        if (alive) {
          setConductorName(s.conductor);
          setConductorTasks(s.tasks);
          setConductorHalted(s.halted);
        }
      } catch {
        /* backend not ready */
      }
    };
    refresh();
    const unlisten = listen("conductor-changed", refresh);
    const poll = setInterval(refresh, 3000);
    return () => {
      alive = false;
      clearInterval(poll);
      unlisten.then((f) => f());
    };
  }, []);

  async function toggleConductor(id: string) {
    const next = conductor === id ? null : id;
    setConductorName(next);
    await setConductor(next).catch(() => {});
  }

  // Keep the backend's context directory in step with the active project —
  // on startup (restoring the remembered one) and on every change.
  useEffect(() => {
    setProjectDir(project).catch(() => {});
  }, [project]);

  async function pickProject() {
    try {
      const dir = await open({
        directory: true,
        title: "Pick the git repo Mosaic sessions should work in",
      });
      if (typeof dir === "string") {
        setProject(dir);
        try {
          localStorage.setItem("mosaic.project", dir);
        } catch {
          /* ignore */
        }
      }
    } catch {
      /* cancelled */
    }
  }

  // Ordered list of brains that currently have panes, "main" always first.
  const brainList = useMemo(() => {
    const set = [...new Set(panes.map((p) => p.brain))];
    set.sort((a, b) => (a === "main" ? -1 : b === "main" ? 1 : 0));
    return set;
  }, [panes]);
  const colorMap = useMemo(() => brainColorMap(brainList), [brainList]);

  function addSession(type: SessionType, isolate = false) {
    const id = `sess-${++counter.current}`;
    setPanes((p) => [...p, { id, type, status: "running", brain: "main", isolate }]);
    setAgentBrain(id, "main").catch(() => {});
    setLauncherOpen(false);
  }

  function closePane(id: string) {
    killSession(id).catch(() => {});
    setPanes((p) => p.filter((x) => x.id !== id));
    setFocusedPane((focused) => (focused === id ? null : focused));
  }

  useEffect(() => {
    try {
      localStorage.setItem("mosaic.layout", layout);
      localStorage.setItem("mosaic.layout.columns", String(layoutColumns));
      localStorage.setItem("mosaic.layout.paneHeight", String(paneHeight));
    } catch {
      /* ignore */
    }
  }, [layout, layoutColumns, paneHeight]);

  function markExited(id: string) {
    setPanes((p) => p.map((x) => (x.id === id ? { ...x, status: "exited" } : x)));
  }

  function setPaneIsolation(id: string, isolate: boolean) {
    setPanes((p) => p.map((x) => (x.id === id ? { ...x, isolate } : x)));
  }

  // Re-home a pane (and its agent) to a brain. The backend re-scopes on the
  // agent's next tool call.
  function assignBrain(paneId: string, brain: string) {
    setPanes((p) => p.map((x) => (x.id === paneId ? { ...x, brain } : x)));
    setAgentBrain(paneId, brain).catch(() => {});
  }

  function nextBrainName() {
    let n = 2;
    while (brainList.includes(`brain-${n}`)) n++;
    return `brain-${n}`;
  }

  // ---- Dispatch ----
  async function handleDispatch(target: string, task: string) {
    await dispatchTask(target, task);
  }

  function dismissToast(taskId: string) {
    setToasts((t) => [...t, taskId]);
  }

  // ---- drag wiring ----
  // Chromium needs a real dataTransfer payload on dragstart and an explicit
  // dropEffect on dragover, or it shows the "no drop" cursor even when the
  // target would accept. (Tauri's native dragDropEnabled must also be off.)
  function onDragStart(e: DragEvent, id: string) {
    dragId.current = id;
    e.dataTransfer.setData("text/plain", id);
    e.dataTransfer.effectAllowed = "move";
  }
  function allowDrop(e: DragEvent) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  }
  function handleDropOnBrain(brain: string, draggedId?: string) {
    const id = draggedId || dragId.current;
    if (id) assignBrain(id, brain);
    dragId.current = null;
  }
  function handleDropOnNewBrain(draggedId?: string) {
    const id = draggedId || dragId.current;
    if (id) assignBrain(id, nextBrainName());
    dragId.current = null;
  }
  function onDropPane(e: DragEvent, brain: string) {
    e.preventDefault();
    handleDropOnBrain(brain, e.dataTransfer.getData("text/plain"));
  }

  // Capture app shortcuts before xterm sees them. Plain Ctrl+K/Ctrl+B belong
  // to terminal applications, so Mosaic uses Ctrl+Shift chords instead.
  useEffect(() => {
    const focusPaneAt = (index: number) => {
      const pane = document.querySelectorAll<HTMLElement>(".pane")[index];
      if (!pane) return;
      const id = pane.dataset.paneId;
      // While a pane is maximized, move the spotlight to the requested one rather
      // than dropping out of focus mode. The rail button beside this shortcut
      // switches panes, and its tooltip advertises the shortcut as the same
      // action, so exiting instead made the two disagree. Outside focus mode
      // there is no spotlight to move and the rAF below just focuses the terminal.
      if (id) setFocusedPane((current) => (current ? id : current));
      requestAnimationFrame(() =>
        requestAnimationFrame(() =>
          pane.querySelector<HTMLElement>(".xterm-helper-textarea")?.focus(),
        ),
      );
    };
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        e.stopPropagation();
        setLauncherOpen((o) => !o);
      } else if (mod && e.shiftKey && e.key === ",") {
        e.preventDefault();
        e.stopPropagation();
        setSettingsOpen((o) => !o);
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "b") {
        e.preventDefault();
        e.stopPropagation();
        setSidebarOpen((o) => !o);
      } else if (mod && e.shiftKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        e.stopPropagation();
        focusPaneAt(Number(e.key) - 1);
      } else if (mod && e.shiftKey && e.key === "Enter") {
        const pane = document.activeElement?.closest<HTMLElement>(".pane");
        const id = pane?.dataset.paneId;
        if (!id) return;
        e.preventDefault();
        e.stopPropagation();
        setFocusedPane((current) => (current === id ? null : id));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Same-brain panes cluster together; order is stable so drags don't remount.
  const ordered = useMemo(
    () => [...panes].sort((a, b) => brainList.indexOf(a.brain) - brainList.indexOf(b.brain)),
    [panes, brainList],
  );
  const count = Math.min(panes.length, 6);
  const grouped = brainList.length > 1;

  return (
    <div className="app">
      <header className="bar">
        <h1>Mosaic</h1>
        <span className="tag">cockpit</span>
        <div className="spacer" />
        <span className="count">
          {panes.length} session{panes.length === 1 ? "" : "s"}
          {grouped ? ` · ${brainList.length} brains` : ""}
        </span>
        <button
          className={"ghost" + (project ? " on" : "")}
          onClick={pickProject}
          title={project ?? "No project set — sessions run where the app launched"}
        >
          {project ? `⌂ ${project.split(/[\\/]/).filter(Boolean).pop()}` : "Pick project"}
        </button>
        <button
          className={"ghost" + (sidebarOpen ? " on" : "")}
          onClick={() => setSidebarOpen((o) => !o)}
          title="Shared brain (Ctrl+Shift+B)"
        >
          Shared brain
        </button>
        <button
          className={"ghost" + (layout === "scroll" ? " on" : "")}
          onClick={() => setLayoutOpen(true)}
          title="Customize terminal layout"
        >
          Layout: {layoutColumns === "auto" ? "auto" : `${layoutColumns} col`}
        </button>
        <button
          className="ghost"
          onClick={() => setSettingsOpen(true)}
          title="Appearance (Ctrl+Shift+,)"
        >
          Appearance
        </button>
        <button className="primary" onClick={() => setLauncherOpen(true)}>
          + New session <kbd>Ctrl Shift K</kbd>
        </button>
      </header>

      {restoreProblems.length > 0 && (
        <div className="restore-problem" role="status">
          <span>{restoreProblems.join(" · ")}</span>
          <div className="spacer" />
          <button className="ghost" onClick={() => setRestoreProblems([])}>
            Dismiss
          </button>
        </div>
      )}

      {conductor && (
        <ConductorBar
          conductor={conductor}
          tasks={conductorTasks}
          halted={conductorHalted}
          onDemote={() => toggleConductor(conductor)}
          onHaltChange={haltConductor}
          onOpenDispatch={() => setDispatchOpen(true)}
          panes={panes}
        />
      )}

      <div className="body">
        {focusedPane && (
          <nav className="spotlight-rail" aria-label="Live sessions">
            {ordered.map((p, index) => (
              <button
                key={p.id}
                className={"spotlight-item" + (focusedPane === p.id ? " active" : "")}
                onClick={() => setFocusedPane(p.id)}
                title={`Switch to ${p.type.label} ${p.id} (Ctrl+Shift+${index + 1})`}
              >
                <span className="dot" style={{ background: p.type.color }} />
                <span>{p.type.label}</span>
                <small>{index + 1}</small>
              </button>
            ))}
          </nav>
        )}
        <main
          className="grid"
          data-count={count}
          data-layout={layout}
          data-columns={layoutColumns}
          data-focused={focusedPane !== null}
          style={
            {
              "--layout-columns": layoutColumns === "auto" ? 1 : layoutColumns,
              "--pane-min-height": `${paneHeight}px`,
            } as CSSProperties
          }
        >
          {panes.length === 0 ? (
            <div className="empty">
              <div className="empty-title">No sessions yet</div>
              <div className="empty-sub">
                Open live agents in panes, then drag them together — onto each
                other or a brain in the sidebar — to share context.
              </div>
              <button className="primary" onClick={() => setLauncherOpen(true)}>
                + New session <kbd>Ctrl Shift K</kbd>
              </button>
            </div>
          ) : (
            ordered.map((p) => {
              const color = colorMap[p.brain];
              return (
                <section
                  className={
                    "pane" +
                    (grouped ? " grouped" : "") +
                    (focusedPane === p.id ? " focused" : "")
                  }
                  key={p.id}
                  data-pane-id={p.id}
                  data-status={p.status}
                  style={grouped ? ({ "--brain": color } as CSSProperties) : undefined}
                  onDragOver={allowDrop}
                  onDrop={(e) => onDropPane(e, p.brain)}
                >
                  <div
                    className="pane-head"
                    draggable
                    onDragStart={(e) => onDragStart(e, p.id)}
                    title="Drag onto another pane or a brain to connect"
                  >
                    <span className="dot" style={{ background: p.type.color }} />
                    <span className="pane-label">{p.type.label}</span>
                    <span className="pane-id">{p.id}</span>
                    {grouped && (
                      <span className="brain-chip" style={{ background: color, color: "#16161e" }}>
                        {p.brain}
                      </span>
                    )}
                    {p.isolate && (
                      <span className="pane-iso" title="Runs in its own git worktree + branch">
                        ⑂ isolated
                      </span>
                    )}
                    {p.status === "exited" && <span className="pane-ended">ended</span>}
                    <div className="spacer" />
<button
          className={"pane-cond" + (conductor === p.id ? " on" : "")}
          title={
            conductor === p.id
              ? "Conductor — click to demote"
              : "Make this pane the conductor"
          }
          aria-label={conductor === p.id ? "Demote conductor" : "Make conductor"}
          onClick={() => toggleConductor(p.id)}
        >
          ⌁
        </button>
                    <button
                      className={"pane-focus" + (focusedPane === p.id ? " on" : "")}
                      title={focusedPane === p.id ? "Show all panes" : "Maximize this pane"}
                      aria-pressed={focusedPane === p.id}
                      onClick={() => setFocusedPane((current) => (current === p.id ? null : p.id))}
                    >
                      {focusedPane === p.id ? "Restore" : "Maximize"}
                    </button>
<button className="pane-x" title="Close session" aria-label="Close session" onClick={() => closePane(p.id)}>
          ✕
        </button>
                  </div>
                  <TerminalPane
                    sessionId={p.id}
                    type={p.type}
                    isolate={p.isolate}
                    cwd={project ?? undefined}
                    reuseWorktree={p.worktree}
                    onExit={markExited}
                    onIsolationChange={setPaneIsolation}
                    onSpawnError={noteSpawnFailure}
                  />
                </section>
              );
            })
          )}
        </main>

        {sidebarOpen && (
          <ContextSidebar
            brains={brainList}
            colorMap={colorMap}
            panes={panes.map((p) => ({ id: p.id, brain: p.brain, label: p.type.label }))}
            selectedBrain={selectedBrain}
            onSelectBrain={setSelectedBrain}
            onDropBrain={handleDropOnBrain}
            onDropNewBrain={handleDropOnNewBrain}
            onClose={() => setSidebarOpen(false)}
          />
        )}
      </div>

      {launcherOpen && (
        <SessionLauncher
          project={project}
          onPick={addSession}
          onClose={() => setLauncherOpen(false)}
        />
      )}
      {settingsOpen && <SettingsPanel onClose={() => setSettingsOpen(false)} />}
      {layoutOpen && (
        <LayoutPanel
          mode={layout}
          columns={layoutColumns}
          paneHeight={paneHeight}
          onMode={setLayout}
          onColumns={setLayoutColumns}
          onPaneHeight={setPaneHeight}
          onClose={() => setLayoutOpen(false)}
        />
      )}
      {dispatchOpen && (
        <DispatchDialog
          panes={panes}
          conductorId={conductor}
          onClose={() => setDispatchOpen(false)}
          onDispatch={handleDispatch}
        />
      )}
      <ToastContainer
        tasks={conductorTasks.filter((t) => !toasts.includes(t.id))}
        onDismiss={dismissToast}
      />
    </div>
  );
}

export default App;
