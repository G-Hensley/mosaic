import { useEffect } from "react";

export type LayoutMode = "scroll" | "fit";
export type LayoutColumns = "auto" | 1 | 2 | 3 | 4 | 5 | 6;

export function LayoutPanel({
  mode,
  columns,
  paneHeight,
  onMode,
  onColumns,
  onPaneHeight,
  onClose,
}: {
  mode: LayoutMode;
  columns: LayoutColumns;
  paneHeight: number;
  onMode: (mode: LayoutMode) => void;
  onColumns: (columns: LayoutColumns) => void;
  onPaneHeight: (height: number) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings layout-settings" onClick={(e) => e.stopPropagation()}>
        <div className="settings-head">
          <span className="settings-title">Terminal layout</span>
          <button className="settings-x" onClick={onClose} title="Close" aria-label="Close">
            ✕
          </button>
        </div>

        <div className="settings-section">Behavior</div>
        <div className="seg">
          {(["scroll", "fit"] as const).map((value) => (
            <button
              key={value}
              className={"seg-btn" + (mode === value ? " on" : "")}
              onClick={() => onMode(value)}
            >
              {value === "scroll" ? "Scrollable" : "Fit window"}
            </button>
          ))}
        </div>
        <p className="layout-help">
          Scrollable preserves terminal size. Fit window keeps every session visible.
        </p>

        <div className="settings-section">Columns</div>
        <div className="seg layout-columns">
          {(["auto", 1, 2, 3, 4, 5, 6] as const).map((value) => (
            <button
              key={value}
              className={"seg-btn" + (columns === value ? " on" : "")}
              onClick={() => onColumns(value)}
            >
              {value === "auto" ? "Auto" : value}
            </button>
          ))}
        </div>

        <div className="settings-section layout-slider-head">
          <span>Minimum pane height</span>
          <span className="layout-value">{paneHeight}px</span>
        </div>
        <input
          className="layout-slider"
          type="range"
          min="280"
          max="720"
          step="20"
          value={paneHeight}
          disabled={mode === "fit"}
          onChange={(e) => onPaneHeight(Number(e.currentTarget.value))}
        />
        <div className="layout-scale">
          <span>Compact</span>
          <span>Spacious</span>
        </div>
        <p className="layout-help layout-shortcuts">
          Ctrl+Shift+1…9 focuses a pane. Ctrl+Shift+Enter maximizes or restores
          the terminal you are using.
        </p>
      </div>
    </div>
  );
}
