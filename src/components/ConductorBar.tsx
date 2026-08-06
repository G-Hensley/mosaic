import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { conductorState, haltConductor, type ConductorTask } from "../lib/ipc";

// Live view of what the conductor is doing, plus the global kill-switch.
// Every dispatch also lands visibly in its target's terminal — this bar is the
// at-a-glance version.
export function ConductorBar({
  conductor,
  onDemote,
}: {
  conductor: string;
  onDemote: () => void;
}) {
  const [tasks, setTasks] = useState<ConductorTask[]>([]);
  const [halted, setHalted] = useState(false);

  useEffect(() => {
    let alive = true;
    const refresh = async () => {
      try {
        const s = await conductorState();
        if (!alive) return;
        setTasks(s.tasks);
        setHalted(s.halted);
      } catch {
        /* backend not ready */
      }
    };
    refresh();
    const unlisten = listen("conductor-changed", refresh);
    const poll = setInterval(refresh, 3000); // catch agent-side completions
    return () => {
      alive = false;
      clearInterval(poll);
      unlisten.then((f) => f());
    };
  }, []);

  const pending = tasks.filter((t) => t.status === "pending").length;

  return (
    <div className="condbar" data-halted={halted}>
      <span className="cond-title">
        ⌁ Conductor <b>{conductor}</b>
      </span>
      <span className="cond-count">
        {pending} pending · {tasks.length} dispatched
      </span>

      <div className="cond-feed">
        {tasks.length === 0 ? (
          <span className="cond-empty">No dispatches yet</span>
        ) : (
          tasks
            .slice()
            .reverse()
            .slice(0, 5)
            .map((t) => (
              <span
                className="cond-task"
                key={t.id}
                data-status={t.status}
                title={t.result || t.task}
              >
                <b>{t.target}</b> {t.task.length > 38 ? t.task.slice(0, 38) + "…" : t.task}
                <i>{t.status}</i>
              </span>
            ))
        )}
      </div>

      <div className="spacer" />
      <button className="ghost" onClick={onDemote} title="Stop being the conductor">
        Demote
      </button>
      <button
        className={"stop" + (halted ? " on" : "")}
        onClick={() => haltConductor(!halted)}
        title={halted ? "Resume dispatching" : "Halt all dispatch immediately"}
      >
        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
          <circle cx="12" cy="12" r="9.5" fill="none" stroke="currentColor" strokeWidth="2" />
          <rect x="8.5" y="8.5" width="7" height="7" rx="1.6" fill="currentColor" />
        </svg>
        {halted ? "Resume" : "Stop"}
      </button>
    </div>
  );
}
