import { useMemo } from "react";
import { type ConductorTask } from "../lib/ipc";

type ConductorBarProps = {
  conductor: string;
  tasks: ConductorTask[];
  halted: boolean;
  onDemote: () => void;
  onHaltChange: (halted: boolean) => void;
  onOpenDispatch: () => void;
  panes: { id: string; type: import("../lib/ipc").SessionType; status: string }[];
};

// Live view of what the conductor is doing, plus the global kill-switch.
// Every dispatch also lands visibly in its target's terminal — this bar is the
// at-a-glance version.
export function ConductorBar({
  conductor,
  tasks,
  halted,
  onDemote,
  onHaltChange,
  onOpenDispatch,
  panes,
}: ConductorBarProps) {
  const availableTargets = useMemo(
    () => panes.filter((p) => p.status === "running" && p.id !== conductor && p.type.id !== "shell"),
    [panes, conductor]
  );

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
      {availableTargets.length > 0 && (
        <button className="ghost" onClick={onOpenDispatch} title="Dispatch a task to an agent">
          Dispatch…
        </button>
      )}
      <button className="ghost" onClick={onDemote} title="Stop being the conductor">
        Demote
      </button>
      <button
        className={"stop" + (halted ? " on" : "")}
        onClick={() => onHaltChange(!halted)}
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
