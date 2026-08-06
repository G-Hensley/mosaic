// The theme registry. A theme is ONE data block: a set of chrome tokens (applied
// as CSS custom properties) + a matching xterm palette (applied to each live
// terminal). Adding a theme = appending one entry here.

export const TERM_FONT =
  "'JetBrains Mono', 'Cascadia Code', Menlo, Consolas, monospace";

export type Tokens = {
  bg: string; // app background
  bar: string; // title bar background
  panel: string; // pane background
  panel2: string; // pane header / launcher / settings surface
  edge: string; // borders
  txt: string; // primary text
  dim: string; // muted text
  acc: string; // accent / primary button
  accInk: string; // text on accent
  danger: string; // destructive (close, errors)
  sel: string; // hover / selection tint (rgba)
};

export type XtermTheme = {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
};

export type Theme = {
  id: string;
  name: string;
  light?: boolean;
  glow?: boolean; // prefers the neon glow accent (SynthWave)
  tokens: Tokens;
  xterm: XtermTheme;
};

export const THEMES: Theme[] = [
  {
    id: "tokyo-night",
    name: "Tokyo Night",
    tokens: {
      bg: "#16161e", bar: "#1a1b26", panel: "#1a1b26", panel2: "#1c1d2b",
      edge: "#2a2e42", txt: "#c0caf5", dim: "#565f89", acc: "#7aa2f7",
      accInk: "#16161e", danger: "#f7768e", sel: "rgba(122,162,247,0.14)",
    },
    xterm: {
      background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5",
      cursorAccent: "#1a1b26", selectionBackground: "#33467c",
      black: "#15161e", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68",
      blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#a9b1d6",
      brightBlack: "#414868", brightRed: "#f7768e", brightGreen: "#9ece6a",
      brightYellow: "#e0af68", brightBlue: "#7aa2f7", brightMagenta: "#bb9af7",
      brightCyan: "#7dcfff", brightWhite: "#c0caf5",
    },
  },
  {
    id: "synthwave-84",
    name: "SynthWave '84",
    glow: true,
    tokens: {
      bg: "#262335", bar: "#241b2f", panel: "#241b2f", panel2: "#2a2139",
      edge: "#3b2f4a", txt: "#ffffff", dim: "#a290c0", acc: "#ff7edb",
      accInk: "#241b2f", danger: "#fe4450", sel: "rgba(255,126,219,0.16)",
    },
    xterm: {
      background: "#262335", foreground: "#ffffff", cursor: "#f97e72",
      cursorAccent: "#262335", selectionBackground: "#463465",
      black: "#241b2f", red: "#fe4450", green: "#72f1b8", yellow: "#fede5d",
      blue: "#03edf9", magenta: "#ff7edb", cyan: "#03edf9", white: "#ffffff",
      brightBlack: "#495495", brightRed: "#fe4450", brightGreen: "#72f1b8",
      brightYellow: "#fede5d", brightBlue: "#03edf9", brightMagenta: "#ff7edb",
      brightCyan: "#03edf9", brightWhite: "#ffffff",
    },
  },
  {
    id: "nord",
    name: "Nord",
    tokens: {
      bg: "#2e3440", bar: "#2b303b", panel: "#2e3440", panel2: "#343b4a",
      edge: "#3b4252", txt: "#d8dee9", dim: "#7b869c", acc: "#88c0d0",
      accInk: "#2e3440", danger: "#bf616a", sel: "rgba(136,192,208,0.16)",
    },
    xterm: {
      background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9",
      cursorAccent: "#2e3440", selectionBackground: "#434c5e",
      black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
      blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
      brightBlack: "#4c566a", brightRed: "#bf616a", brightGreen: "#a3be8c",
      brightYellow: "#ebcb8b", brightBlue: "#81a1c1", brightMagenta: "#b48ead",
      brightCyan: "#8fbcbb", brightWhite: "#eceff4",
    },
  },
  {
    id: "everforest",
    name: "Everforest",
    tokens: {
      bg: "#2d353b", bar: "#2b3339", panel: "#2d353b", panel2: "#343f44",
      edge: "#3d484d", txt: "#d3c6aa", dim: "#859289", acc: "#a7c080",
      accInk: "#2d353b", danger: "#e67e80", sel: "rgba(167,192,128,0.16)",
    },
    xterm: {
      background: "#2d353b", foreground: "#d3c6aa", cursor: "#d3c6aa",
      cursorAccent: "#2d353b", selectionBackground: "#475258",
      black: "#343f44", red: "#e67e80", green: "#a7c080", yellow: "#dbbc7f",
      blue: "#7fbbb3", magenta: "#d699b6", cyan: "#83c092", white: "#d3c6aa",
      brightBlack: "#859289", brightRed: "#e67e80", brightGreen: "#a7c080",
      brightYellow: "#dbbc7f", brightBlue: "#7fbbb3", brightMagenta: "#d699b6",
      brightCyan: "#83c092", brightWhite: "#e8e0cd",
    },
  },
  {
    id: "rose-pine",
    name: "Rosé Pine",
    tokens: {
      bg: "#191724", bar: "#1f1d2e", panel: "#1f1d2e", panel2: "#26233a",
      edge: "#2a2739", txt: "#e0def4", dim: "#6e6a86", acc: "#ebbcba",
      accInk: "#191724", danger: "#eb6f92", sel: "rgba(235,188,186,0.14)",
    },
    xterm: {
      background: "#191724", foreground: "#e0def4", cursor: "#e0def4",
      cursorAccent: "#191724", selectionBackground: "#403d52",
      black: "#26233a", red: "#eb6f92", green: "#31748f", yellow: "#f6c177",
      blue: "#9ccfd8", magenta: "#c4a7e7", cyan: "#ebbcba", white: "#e0def4",
      brightBlack: "#6e6a86", brightRed: "#eb6f92", brightGreen: "#31748f",
      brightYellow: "#f6c177", brightBlue: "#9ccfd8", brightMagenta: "#c4a7e7",
      brightCyan: "#ebbcba", brightWhite: "#e0def4",
    },
  },
  {
    id: "paper",
    name: "Paper (light)",
    light: true,
    tokens: {
      bg: "#f5f3ee", bar: "#efeae0", panel: "#ffffff", panel2: "#f0ece3",
      edge: "#dcd6ca", txt: "#33312c", dim: "#8a8577", acc: "#3b7dd8",
      accInk: "#ffffff", danger: "#c0392b", sel: "rgba(59,125,216,0.12)",
    },
    xterm: {
      background: "#ffffff", foreground: "#33312c", cursor: "#33312c",
      cursorAccent: "#ffffff", selectionBackground: "#cfe0f8",
      black: "#33312c", red: "#c0392b", green: "#3a7d3a", yellow: "#b7791f",
      blue: "#3b7dd8", magenta: "#9b59b6", cyan: "#2a9d8f", white: "#d8d3c8",
      brightBlack: "#8a8577", brightRed: "#c0392b", brightGreen: "#3a7d3a",
      brightYellow: "#b7791f", brightBlue: "#3b7dd8", brightMagenta: "#9b59b6",
      brightCyan: "#2a9d8f", brightWhite: "#33312c",
    },
  },
];

export const DEFAULT_THEME_ID = "tokyo-night";

export function resolveTheme(id: string): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}
