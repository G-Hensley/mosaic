import { useEffect, useMemo, useRef, useState, type CSSProperties, type DragEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { TerminalPane } from "./components/TerminalPane";
import { SessionLauncher } from "./components/SessionLauncher";
import { SettingsPanel } from "./components/SettingsPanel";
import { ContextSidebar } from "./components/ContextSidebar";
import { ConductorBar } from "./components/ConductorBar";
import {
  killSession,
  setAgentBrain,
  conductorState,
  setConductor,
  setProject as setProjectDir,
  type SessionType,
} from "./lib/ipc";
import { brainColorMap } from "./lib/brains";
import "./App.css";

type PaneStatus = "running" | "exited";
type LayoutMode = "scroll" | "fit";
type Pane = {
  id: string;
  type: SessionType;
  status: PaneStatus;
  brain: string;
  isolate: boolean;
};

function App() {
  const [panes, setPanes] = useState<Pane[]>([]);
  const [launcherOpen, setLauncherOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [layout, setLayout] = useState<LayoutMode>(() => {
    try {
      return localStorage.getItem("mosaic.layout") === "fit" ? "fit" : "scroll";
    } catch {
      return "scroll";
    }
  });
  const [focusedPane, setFocusedPane] = useState<string | null>(null);
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
  const counter = useRef(0);
  const dragId = useRef<string | null>(null);

  useEffect(() => {
    let alive = true;
    const refresh = async () => {
      try {
        const s = await conductorState();
        if (alive) setConductorName(s.conductor);
      } catch {
        /* backend not ready */
      }
    };
    refresh();
    const unlisten = listen("conductor-changed", refresh);
    return () => {
      alive = false;
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

  function toggleLayout() {
    setLayout((current) => {
      const next = current === "scroll" ? "fit" : "scroll";
      try {
        localStorage.setItem("mosaic.layout", next);
      } catch {
        /* ignore */
      }
      return next;
    });
  }

  function markExited(id: string) {
    setPanes((p) => p.map((x) => (x.id === id ? { ...x, status: "exited" } : x)));
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
          onClick={toggleLayout}
          title={
            layout === "scroll"
              ? "Scrollable comfortable panes — click to fit all"
              : "Fit every pane on screen — click for comfortable scrolling"
          }
        >
          Layout: {layout === "scroll" ? "scroll" : "fit"}
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

      {conductor && (
        <ConductorBar conductor={conductor} onDemote={() => toggleConductor(conductor)} />
      )}

      <div className="body">
        <main
          className="grid"
          data-count={count}
          data-layout={layout}
          data-focused={focusedPane !== null}
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
                    <button className="pane-x" title="Close session" onClick={() => closePane(p.id)}>
                      ✕
                    </button>
                  </div>
                  <TerminalPane
                    sessionId={p.id}
                    type={p.type}
                    isolate={p.isolate}
                    cwd={project ?? undefined}
                    onExit={markExited}
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
        <SessionLauncher onPick={addSession} onClose={() => setLauncherOpen(false)} />
      )}
      {settingsOpen && <SettingsPanel onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}

export default App;
