// Remembering which agent CLIs were open, so reopening Mosaic brings the
// workspace back rather than an empty grid.
//
// The project directory and the layout already survive a restart; the panes did
// not, so every agent had to be relaunched by hand. This is the roster half of
// that: what each pane *was*, not what it was doing. Nothing about a live
// session is stored — a restored pane is freshly spawned, so its status comes
// from the new spawn, and its scrollback is gone with the old process.
//
// localStorage, alongside `mosaic.project` and `mosaic.layout*`, rather than the
// Rust store: the roster is UI state the frontend owns end to end, it is written
// on every pane change, and keeping it beside the project it belongs with means
// one place to look when a restore misbehaves.

import { SESSION_TYPES, type SavedWorktree, type SessionType } from "./ipc";

const KEY = "mosaic.panes";

// What is persisted per pane. The session *type id* is stored rather than the
// type object, so a pane restored after an upgrade picks up the current
// program, args and colour instead of resurrecting stale ones.
export type StoredPane = {
  id: string;
  typeId: string;
  brain: string;
  isolate: boolean;
  worktree?: SavedWorktree;
};

// A stored pane with its session type resolved — ready to become a live pane.
export type RestoredPane = Omit<StoredPane, "typeId"> & { type: SessionType };

export type Roster = {
  panes: RestoredPane[];
  // One message per entry that could not be restored. A bad entry is dropped
  // from the restore, never allowed to throw, and never a reason to discard the
  // rest of the roster.
  problems: string[];
};

const EMPTY: Roster = { panes: [], problems: [] };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

// A worktree reference is only useful if every field survived; a partial one
// would send the backend looking for a directory it cannot identify.
function parseWorktree(value: unknown): SavedWorktree | undefined {
  if (!isRecord(value)) return undefined;
  const { repo, path, branch, base } = value;
  if (
    typeof repo !== "string" ||
    typeof path !== "string" ||
    typeof branch !== "string" ||
    typeof base !== "string" ||
    !path
  ) {
    return undefined;
  }
  return { repo, path, branch, base };
}

// Turn stored JSON into panes, reporting rather than throwing on anything it
// cannot use. Pure, so the parsing rules can be reasoned about (and exercised)
// without a browser.
export function parseRoster(raw: string | null): Roster {
  if (!raw) return EMPTY;
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    // Corrupt roster: start empty rather than blocking startup. The file is
    // rewritten from live state as soon as anything changes.
    return { panes: [], problems: ["Saved sessions could not be read, so none were restored."] };
  }
  if (!Array.isArray(data)) return EMPTY;

  const panes: RestoredPane[] = [];
  const problems: string[] = [];
  const seen = new Set<string>();

  for (const entry of data) {
    if (!isRecord(entry)) continue;
    const id = typeof entry.id === "string" ? entry.id : "";
    if (!id || seen.has(id)) continue;
    const type = SESSION_TYPES.find((t) => t.id === entry.typeId);
    if (!type) {
      // The session type is gone from this build — nothing to launch.
      problems.push(`Could not restore ${id}: its session type is no longer available.`);
      continue;
    }
    seen.add(id);
    panes.push({
      id,
      type,
      brain: typeof entry.brain === "string" && entry.brain ? entry.brain : "main",
      isolate: entry.isolate === true,
      worktree: parseWorktree(entry.worktree),
    });
  }
  return { panes, problems };
}

// Read the remembered roster. Never throws: a browser that denies localStorage
// simply starts with no sessions.
export function loadRoster(): Roster {
  try {
    return parseRoster(localStorage.getItem(KEY));
  } catch {
    return EMPTY;
  }
}

export function saveRoster(panes: StoredPane[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(panes));
  } catch {
    /* ignore — persistence is best-effort, it must never break the app */
  }
}

// Where the `sess-N` counter has to resume from.
//
// Restored panes already hold their old ids, so a counter starting at 0 would
// hand the next session an id that is still live — the backend refuses the
// duplicate and the new pane never starts. Seeding past the highest restored
// number keeps new ids clear of them.
export function seedCounter(ids: string[]): number {
  let highest = 0;
  for (const id of ids) {
    const match = /^sess-(\d+)$/.exec(id);
    if (match) highest = Math.max(highest, Number(match[1]));
  }
  return highest;
}
