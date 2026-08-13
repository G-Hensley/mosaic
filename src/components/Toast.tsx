import { useEffect, useRef, useState } from "react";
import { type ConductorTask } from "../lib/ipc";

type ToastProps = {
  task: ConductorTask;
  onDismiss: (id: string) => void;
};

function Toast({ task, onDismiss }: ToastProps) {
  const [visible, setVisible] = useState(true);

  // Held in a ref so polling that hands us a fresh `task` object or a new
  // `onDismiss` identity cannot restart the countdown mid-flight.
  const onDismissRef = useRef(onDismiss);
  useEffect(() => {
    onDismissRef.current = onDismiss;
  }, [onDismiss]);

  useEffect(() => {
    let fadeTimer: ReturnType<typeof setTimeout> | undefined;
    const timer = setTimeout(() => {
      setVisible(false);
      fadeTimer = setTimeout(() => onDismissRef.current(task.id), 300);
    }, 6000);
    return () => {
      clearTimeout(timer);
      if (fadeTimer !== undefined) clearTimeout(fadeTimer);
    };
  }, [task.id]);

  if (!visible) return null;

  const isDone = task.status === "done";
  // "overdue" is deliberately absent: the agent is still working and its
  // result is still accepted, so showing it as a failure would tell the
  // conductor to give up on work that is about to arrive.
  const isError = task.status === "error" || task.status === "cancelled";

  return (
    <div
      className={"toast" + (isDone ? " done" : isError ? " error" : " pending")}
      role="status"
    >
      <div className="toast-icon" aria-hidden="true">
        {isDone ? "✓" : isError ? "✕" : "⟳"}
      </div>
      <div className="toast-content">
        <div className="toast-header">
          <span className="toast-target">{task.target}</span>
          <span className="toast-status">{task.status}</span>
        </div>
        <div className="toast-task">{task.task}</div>
        {task.result && (
          <div className="toast-result">{task.result.slice(0, 120)}</div>
        )}
      </div>
      <button
        className="toast-dismiss"
        onClick={() => onDismiss(task.id)}
        aria-label="Dismiss"
      >
        ✕
      </button>
    </div>
  );
}

export function ToastContainer({
  tasks,
  onDismiss,
}: {
  tasks: ConductorTask[];
  onDismiss: (id: string) => void;
}) {
  // Terminal states only. An overdue task has not finished, so surfacing it
  // here would announce a result that does not exist yet.
  const completedTasks = tasks.filter(
    (t) => t.status === "done" || t.status === "error" || t.status === "cancelled"
  );

  return (
    <div className="toast-container" aria-live="polite" aria-label="Task notifications">
      {completedTasks.slice(-3).map((task) => (
        <Toast key={task.id} task={task} onDismiss={onDismiss} />
      ))}
    </div>
  );
}
