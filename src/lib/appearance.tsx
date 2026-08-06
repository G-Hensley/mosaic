import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { DEFAULT_THEME_ID, resolveTheme, type Theme } from "./themes";

export type Appearance = { theme: string; glow: boolean; fontSize: number };

const KEY = "mosaic.appearance";
const DEFAULT: Appearance = { theme: DEFAULT_THEME_ID, glow: false, fontSize: 13 };

export function loadAppearance(): Appearance {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { ...DEFAULT, ...JSON.parse(raw) };
  } catch {
    /* corrupt/unavailable storage → defaults */
  }
  return { ...DEFAULT };
}

function save(a: Appearance) {
  try {
    localStorage.setItem(KEY, JSON.stringify(a));
  } catch {
    /* ignore */
  }
}

// Push a theme's chrome tokens onto the document root as CSS custom properties,
// plus data-* attributes the stylesheet keys off (glow, light).
export function applyAppearance(a: Appearance) {
  const theme = resolveTheme(a.theme);
  const root = document.documentElement;
  const t = theme.tokens;
  root.style.setProperty("--bg", t.bg);
  root.style.setProperty("--bar", t.bar);
  root.style.setProperty("--panel", t.panel);
  root.style.setProperty("--panel2", t.panel2);
  root.style.setProperty("--edge", t.edge);
  root.style.setProperty("--txt", t.txt);
  root.style.setProperty("--dim", t.dim);
  root.style.setProperty("--acc", t.acc);
  root.style.setProperty("--acc-ink", t.accInk);
  root.style.setProperty("--danger", t.danger);
  root.style.setProperty("--sel", t.sel);
  root.dataset.scheme = theme.id;
  root.dataset.glow = String(a.glow);
  root.dataset.light = String(!!theme.light);
}

type Ctx = {
  appearance: Appearance;
  theme: Theme;
  setTheme: (id: string) => void;
  setGlow: (v: boolean) => void;
  setFontSize: (n: number) => void;
};

const AppearanceContext = createContext<Ctx | null>(null);

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [appearance, setAppearance] = useState<Appearance>(loadAppearance);

  useLayoutEffect(() => {
    applyAppearance(appearance);
    save(appearance);
  }, [appearance]);

  const value = useMemo<Ctx>(
    () => ({
      appearance,
      theme: resolveTheme(appearance.theme),
      // Picking a theme adopts that theme's preferred glow (SynthWave → on).
      setTheme: (id) =>
        setAppearance((a) => ({ ...a, theme: id, glow: resolveTheme(id).glow === true })),
      setGlow: (glow) => setAppearance((a) => ({ ...a, glow })),
      setFontSize: (fontSize) => setAppearance((a) => ({ ...a, fontSize })),
    }),
    [appearance],
  );

  return <AppearanceContext.Provider value={value}>{children}</AppearanceContext.Provider>;
}

export function useAppearance(): Ctx {
  const ctx = useContext(AppearanceContext);
  if (!ctx) throw new Error("useAppearance must be used within AppearanceProvider");
  return ctx;
}
