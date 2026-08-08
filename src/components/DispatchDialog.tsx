import { useEffect, useRef, useState } from "react";
import { type SessionType } from "../lib/ipc";

type DispatchDialogProps = {
  panes: { id: string; type: SessionType; status: string }[];
  conductorId: string | null;
  onClose: () => void;
  onDispatch: (target: string, task: string) => Promise<void>;
};

export function DispatchDialog({
  panes,
  conductorId,
  onClose,
  onDispatch,
}: DispatchDialogProps) {
  const [targetId, setTargetId] = useState<string>("");
  const [task, setTask] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const availableTargets = panes.filter(
    (p) => p.status === "running" && p.id !== conductorId && p.type.id !== "shell"
  );

  useEffect(() => {
    if (availableTargets.length > 0 && !targetId) {
      setTargetId(availableTargets[0].id);
    }
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && !sending) {
        handleSubmit();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [availableTargets, targetId, sending, onClose]);

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  async function handleSubmit() {
    if (!targetId || !task.trim() || sending) return;
    setSending(true);
    setError(null);
    try {
      await onDispatch(targetId, task.trim());
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Dispatch failed");
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="dispatch-backdrop" onClick={onClose}>
      <div className="dispatch" onClick={(e) => e.stopPropagation()}>
        <div className="dispatch-head">
          <span className="dispatch-title">Dispatch task</span>
          <button className="dispatch-x" onClick={onClose} title="Close" aria-label="Close">
            ✕
          </button>
        </div>

        <div className="dispatch-body">
          <div className="dispatch-field">
            <label className="dispatch-label" htmlFor="dispatch-target">
              Target
            </label>
            <select
              id="dispatch-target"
              className="dispatch-select"
              value={targetId}
              onChange={(e) => setTargetId(e.target.value)}
              disabled={sending || availableTargets.length === 0}
              aria-describedby={availableTargets.length === 0 ? "dispatch-target-hint" : undefined}
            >
              {availableTargets.length === 0 ? (
                <option value="" disabled>
                  No available targets
                </option>
              ) : (
                availableTargets.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.type.label} {p.id}
                  </option>
                ))
              )}
            </select>
            {availableTargets.length === 0 && (
              <span id="dispatch-target-hint" className="dispatch-hint">
                Only non-conductor agent panes can receive tasks
              </span>
            )}
          </div>

          <div className="dispatch-field">
            <label className="dispatch-label" htmlFor="dispatch-task">
              Task
            </label>
            <textarea
              ref={textareaRef}
              id="dispatch-task"
              className="dispatch-textarea"
              value={task}
              onChange={(e) => setTask(e.target.value)}
              placeholder="Describe the task for the target agent…"
              rows={4}
              disabled={sending}
              aria-describedby={error ? "dispatch-error" : undefined}
            />
            {error && (
              <span id="dispatch-error" className="dispatch-error" role="alert">
                {error}
              </span>
            )}
          </div>
        </div>

        <div className="dispatch-foot">
          <button className="ghost" onClick={onClose} disabled={sending}>
            Cancel
          </button>
          <button
            className={"primary" + (sending ? " sending" : "")}
            onClick={handleSubmit}
            disabled={!targetId || !task.trim() || sending || availableTargets.length === 0}
          >
            {sending ? (
              <>
                <span className="spinner" aria-hidden="true" />
                Sending…
              </>
            ) : (
              "Dispatch"
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
