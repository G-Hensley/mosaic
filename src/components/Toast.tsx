import { useEffect, useState } from "react";
import { type ConductorTask } from "../lib/ipc";

type ToastProps = {
  task: ConductorTask;
  onDismiss: (id: string) => void;
};

function Toast({ task, onDismiss }: ToastProps) {
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    const timer = setTimeout(() => {
      setVisible(false);
      setTimeout(() => onDismiss(task.id), 300);
    }, 6000);
    return () => clearTimeout(timer);
  }, [task, onDismiss]);

  if (!visible) return null;

  const isDone = task.status === "done";
  const isError =
    task.status === "error" || task.status === "timeout" || task.status === "cancelled";

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
  const completedTasks = tasks.filter(
    (t) =>
      t.status === "done" ||
      t.status === "error" ||
      t.status === "timeout" ||
      t.status === "cancelled"
  );

  return (
    <div className="toast-container" aria-live="polite" aria-label="Task notifications">
      {completedTasks.slice(-3).map((task) => (
        <Toast key={task.id} task={task} onDismiss={onDismiss} />
      ))}
    </div>
  );
}
