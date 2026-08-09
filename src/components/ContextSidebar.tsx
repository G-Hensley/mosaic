import { useEffect, useState, type CSSProperties, type DragEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { getContext, mcpInfo, type ContextEntry } from "../lib/ipc";

const KIND_ICON: Record<string, string> = {
  decision: "🧭",
  fact: "📌",
  broadcast: "📣",
};

type PaneRef = { id: string; brain: string; label: string };

export function ContextSidebar({
  brains,
  colorMap,
  panes,
  selectedBrain,
  onSelectBrain,
  onDropBrain,
  onDropNewBrain,
  onClose,
}: {
  brains: string[];
  colorMap: Record<string, string>;
  panes: PaneRef[];
  selectedBrain: string;
  onSelectBrain: (b: string) => void;
  onDropBrain: (b: string, draggedId?: string) => void;
  onDropNewBrain: (draggedId?: string) => void;
  onClose: () => void;
}) {
  const [entries, setEntries] = useState<ContextEntry[]>([]);
  const [port, setPort] = useState<number | null>(null);

  useEffect(() => {
    let alive = true;
    const refresh = async () => {
      try {
        const c = await getContext();
        if (alive) setEntries(c.entries);
      } catch {
        /* backend not ready */
      }
    };
    refresh();
    mcpInfo().then((i) => alive && setPort(i.port)).catch(() => {});
    const unlisten = listen("context-changed", refresh);
    return () => {
      alive = false;
      unlisten.then((f) => f());
    };
  }, []);

  const tabs = brains.length ? brains : ["main"];
  const shown = entries.filter((e) => e.room === selectedBrain);
  const inBrain = panes.filter((p) => p.brain === selectedBrain);
  const allowDrop = (e: DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  };

  return (
    <aside className="side">
      <div className="side-head">
        <span className="side-title">Brains</span>
        {port && <span className="side-port">:{port}</span>}
        <div className="spacer" />
        <button className="side-x" onClick={onClose} title="Hide">
          ✕
        </button>
      </div>

      {/* Brain pills: click to view, drop a pane on one to connect it. */}
      <div className="brain-pills">
        {tabs.map((b) => (
          <button
            key={b}
            className={"brain-pill" + (b === selectedBrain ? " active" : "")}
            style={{ "--bc": colorMap[b] ?? "#7aa2f7" } as CSSProperties}
            onClick={() => onSelectBrain(b)}
            onDragOver={allowDrop}
            onDrop={(e) => {
              e.preventDefault();
              onDropBrain(b, e.dataTransfer.getData("text/plain"));
            }}
            title="Click to view · drop a pane here to connect it"
            aria-label={`Select brain ${b}`}
          >
            <span className="pill-dot" />
            {b}
          </button>
        ))}
        <button
          className="brain-pill new"
          onDragOver={allowDrop}
          onDrop={(e) => {
            e.preventDefault();
            onDropNewBrain(e.dataTransfer.getData("text/plain"));
          }}
          onClick={() => onDropNewBrain()}
          title="Drop a pane here for a new brain"
          aria-label="Create a new brain"
        >
          + new
        </button>
      </div>

      <div className="side-sessions">
        {inBrain.length === 0 ? (
          <span className="side-hint">No panes in this brain</span>
        ) : (
          inBrain.map((p) => (
            <span className="chip" key={p.id} title={p.id}>
              {p.label}
            </span>
          ))
        )}
      </div>

      <div className="side-list">
        {shown.length === 0 ? (
          <div className="side-empty">
            Nothing shared in <b>{selectedBrain}</b> yet. When an agent in this
            brain records a decision, it appears here — and every other agent in
            the same brain can read it.
          </div>
        ) : (
          shown
            .slice()
            .reverse()
            .map((e, i) => (
              <div className="ctx" key={i}>
                <div className="ctx-top">
                  <span className="ctx-kind">
                    {KIND_ICON[e.kind] ?? "•"} {e.kind}
                  </span>
                  <span className="ctx-author">{e.author}</span>
                </div>
                {e.topic && e.kind !== "broadcast" && (
                  <div className="ctx-topic">{e.topic}</div>
                )}
                <div className="ctx-body">{e.body}</div>
              </div>
            ))
        )}
      </div>
    </aside>
  );
}
