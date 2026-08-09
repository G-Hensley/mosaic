import { useEffect } from "react";
import { THEMES } from "../lib/themes";
import { useAppearance } from "../lib/appearance";

// Settings → Appearance. Every control applies immediately (live preview): the
// whole app + all live terminals retint the instant you click a theme.
export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const { appearance, theme, setTheme, setGlow, setFontSize } = useAppearance();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings" onClick={(e) => e.stopPropagation()}>
        <div className="settings-head">
          <span className="settings-title">Appearance</span>
          <button className="settings-x" onClick={onClose} title="Close" aria-label="Close">
            ✕
          </button>
        </div>

        <div className="settings-section">Theme</div>
        <div className="theme-grid">
          {THEMES.map((t) => (
            <button
              key={t.id}
              className={"theme-card" + (t.id === theme.id ? " active" : "")}
              onClick={() => setTheme(t.id)}
            >
              <div
                className="swatch"
                style={{ background: t.tokens.bg, borderColor: t.tokens.edge }}
              >
                <span className="sw-bar" style={{ background: t.tokens.acc }} />
                <span className="sw-dot" style={{ background: t.xterm.green }} />
                <span className="sw-dot" style={{ background: t.xterm.magenta }} />
                <span className="sw-dot" style={{ background: t.xterm.yellow }} />
                <span className="sw-text" style={{ color: t.tokens.txt }}>
                  Aa
                </span>
              </div>
              <span className="theme-name">
                {t.name}
                {t.id === theme.id && <span className="chk"> ✓</span>}
              </span>
            </button>
          ))}
        </div>

        <div className="settings-section">Terminal size</div>
        <div className="seg">
          {[12, 13, 14].map((n) => (
            <button
              key={n}
              className={"seg-btn" + (appearance.fontSize === n ? " on" : "")}
              onClick={() => setFontSize(n)}
            >
              {n}px
            </button>
          ))}
        </div>

        <div className="settings-row">
          <div>
            <div className="row-label">Neon glow</div>
            <div className="row-sub">Best with SynthWave '84</div>
          </div>
          <button
            className={"toggle" + (appearance.glow ? " on" : "")}
            onClick={() => setGlow(!appearance.glow)}
            role="switch"
            aria-checked={appearance.glow}
          >
            <span className="knob" />
          </button>
        </div>
      </div>
    </div>
  );
}
