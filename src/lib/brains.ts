// A brain (room) groups sessions that share context. Colors are assigned by the
// brain's position in the ordered list, so "main" is always the first color.
export const BRAIN_PALETTE = [
  "#7aa2f7", // blue (main)
  "#9ece6a", // green
  "#bb9af7", // magenta
  "#e0af68", // amber
  "#7dcfff", // cyan
  "#f7768e", // red
  "#ff9e64", // orange
  "#b4f9f8", // teal
];

export function brainColorMap(brains: string[]): Record<string, string> {
  const map: Record<string, string> = {};
  brains.forEach((b, i) => {
    map[b] = BRAIN_PALETTE[i % BRAIN_PALETTE.length];
  });
  return map;
}
