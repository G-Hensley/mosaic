# Session Restore Proposal

## Executive Summary

When a user selects a project in Mosaic, they should see the same pane layout with the same agents assigned to the same brains, running in the same worktrees, with the same conductor — as far as that is **honestly** possible. This proposal distinguishes between what can be restored (configuration, identity, topology) and what cannot (the agent process, its conversation context, its internal reasoning state).

**Crucial distinction:** A restored pane is a **fresh agent wearing an old name**. The previous agent's conversation history, context window, and reasoning state died with the process. Pretending otherwise misleads the user.

---

## What Is Worth Restoring (Truth)

| Aspect | Source | Restorable? | Notes |
|--------|--------|-------------|-------|
| **Pane layout** (columns, mode, pane height) | `localStorage` (already) | ✅ Yes | Already persists across app restarts |
| **Project directory** | `localStorage` (already) | ✅ Yes | Already persists |
| **Pane topology**: ordered list of `{id, type/program, brain, isolate, worktree_branch}` | **NEW** — project-scoped file | ✅ Yes | The core of restore |
| **Brain assignment per pane** | `AgentSession` in `brain.jsonl` + new topology | ✅ Yes | Already persisted per session name |
| **Worktree branch name** | `worktree.rs` creates `mosaic/{id}-{uid}` | ✅ Yes | Branch persists in repo; worktree dir in app data |
| **Conductor role** | `Shared.conductor` (in-memory only today) | ✅ Yes | **Currently lost** — must persist |
| **Isolate flag per pane** | Pane state in React | ✅ Yes | Part of pane topology |

---

## What Is NOT Restorable (The Lie)

| Aspect | Why Not |
|--------|---------|
| **Agent process** | PTY + child process gone on app exit |
| **Conversation context / history** | Lives in the agent's memory, not in Mosaic |
| **Agent's internal reasoning state** | Not exposed via MCP; each launch is a fresh context window |
| **Scrollback / terminal buffer** | PTY is gone; xterm.js buffer is frontend-only |
| **In-flight MCP requests** | Server restarts; no durability |
| **Dispatch budget counter** | In-memory; resets per run (acceptable) |

**Honest UI language:** "Restore session layout" not "Restore sessions". The pane shows the agent's *name* and *brain*, but it is a **new process**.

---

## Storage Shape & Location

### Where: Per-project, not global

```
<project>/.mosaic/
├── brain.jsonl          ← existing: entries, AgentSession, Tasks
├── context/             ← existing: markdown mirrors per brain
└── layout.json          ← NEW: pane topology + conductor
```

**Why project-scoped?** The pane layout, worktree branches, and brain assignments are specific to a git repository. `localStorage` is machine-global and loses the mapping when the project moves or is opened on another machine.

### `layout.json` Schema

```json
{
  "version": 1,
  "projectPath": "/absolute/path/to/repo",
  "panes": [
    {
      "id": "sess-1",
      "program": "codex",
      "args": [],
      "brain": "main",
      "isolate": false,
      "worktreeBranch": null
    },
    {
      "id": "sess-2",
      "program": "claude",
      "args": [],
      "brain": "backend",
      "isolate": true,
      "worktreeBranch": "mosaic/sess-2-a1b2c3"
    }
  ],
  "conductor": "sess-1",
  "layout": {
    "mode": "scroll",
    "columns": "auto",
    "paneHeight": 420
  },
  "savedAt": "2026-08-09T14:30:00Z"
}
```

- `program` + `args` replace `type` (SessionType) — more precise for re-spawning
- `worktreeBranch` records the branch name so we can detect/reuse an existing worktree
- `projectPath` enables "project moved" detection
- Version field allows future migration

### Relationship to `AgentSession` in `brain.jsonl`

`AgentSession { name, kind }` already persists per session name. It is the **right foundation** — not a coincidence. The layout file references sessions by `id` (which equals `name`), and `brain.jsonl` provides the `kind` (program) and current `brain` assignment. On restore:

1. Read `layout.json` for topology
2. Cross-reference `brain.jsonl` for `AgentSession.kind` (program) and `brain` (room)
3. If `layout.json` has newer `brain` than `brain.jsonl`, layout wins (user's explicit drag)
4. If `brain.jsonl` has sessions not in `layout.json`, they are orphaned — ignore or offer to add

---

## Restore Flow

### Trigger: Explicit prompt on project select

**Not automatic.** When the user clicks "Pick project" and selects a directory:

1. Backend checks for `<project>/.mosaic/layout.json`
2. If exists and `projectPath` matches (or is close — see failure modes), frontend shows:
   > "Found a previous session layout for this project (3 panes, 2 brains, conductor: sess-1). Restore it?"
   > [Restore] [Start Fresh]

3. On **Restore**:
   - Frontend reads `layout.json`
   - For each pane in order:
     - Call `spawn_session` with `session_id`, `program`, `args`, `isolate`, `cwd` (project or worktree)
     - Backend creates/reuses worktree if `isolate=true`
     - Backend registers session in `Shared` (sets `AgentSession`, brain assignment)
   - After all panes spawned, call `set_conductor` for the recorded conductor
   - Frontend restores layout settings (columns, mode, paneHeight) from `layout.json` or `localStorage`

4. On **Start Fresh**: Delete `layout.json`, proceed normally.

### Spawn-time worktree reuse logic (`worktree.rs`)

When `isolate=true` and `worktreeBranch` is recorded:

1. Check if branch `mosaic/{id}-{uid}` exists in the repo
2. If branch exists:
   - Check if worktree at `app_data_dir()/worktrees/{id}-{uid}` exists
   - If worktree exists and is **clean** → reuse it (fast)
   - If worktree exists and is **dirty** → **prompt user**:
     > "Worktree for sess-2 has uncommitted changes from the previous run. Reuse it, or create a fresh one?"
     > [Reuse Dirty] [Fresh Worktree]
   - If worktree missing but branch exists → recreate worktree from branch
3. If branch missing → create new (normal flow)

**Never silently discard uncommitted work.** The `RefusedDirty` outcome already exists in `worktree::remove`; restore makes it visible.

---

## Failure Modes & Handling

| Failure | Detection | Response |
|---------|-----------|----------|
| **Program not installed** | `spawn_session` fails (PTY spawn error) | Mark pane "exited" with error toast: "`codex` not found in PATH. Install it or change the program in Settings." |
| **Project directory moved** | `layout.json.projectPath` ≠ current project path | In prompt, show: "This layout was saved from a different location (`/old/path`). Restore anyway?" If yes, update `projectPath` on save. |
| **Worktree dirty from previous run** | `worktree::is_dirty()` returns true | Prompt as above — never auto-discard |
| **Session ID collision** | Counter resets to 0 each launch; `sess-1` already in `layout.json` | Use `layout.json` IDs directly — don't use counter for restore. Counter only for *new* panes after restore. |
| **Brain assignment mismatch** | `layout.json` brain ≠ `brain.jsonl` room for that session | Layout wins (explicit user action). Update `Shared.name_to_room` on spawn. |
| **Conductor pane not restored** | Conductor ID in `layout.json` not in pane list | Clear conductor; user promotes manually. Toast: "Previous conductor not in restored layout." |
| **Corrupt/missing `layout.json`** | Parse error or file missing | Treat as "Start Fresh" — no prompt. |

---

## Smallest First Version (MVP)

**Scope:** Restore pane topology + brain assignment + conductor. Defer worktree reuse prompt and program-missing handling to follow-ups.

### Required Changes

1. **Backend (`lib.rs`)**
   - New Tauri command: `save_layout(project_path, panes[], conductor, layout_settings)` → writes `<project>/.mosaic/layout.json`
   - New Tauri command: `load_layout(project_path)` → reads `layout.json` or returns null
   - Modify `spawn_session`: accept `session_id` from caller (already does), use it directly — **don't generate** when restoring
   - Ensure `Shared.set_conductor` persists conductor to `layout.json` on change (or save on each layout change)

2. **Frontend (`App.tsx`)**
   - On project select: call `load_layout`, if found show restore prompt
   - On restore: iterate panes, call `spawn_session` with recorded `id`, `program`, `args`, `isolate`
   - After spawns: call `set_conductor` for recorded conductor
   - On any pane add/close/drag/brain-change/conductor-change: debounced `save_layout`
   - Stop using `counter` ref for IDs during restore; use `layout.json` IDs

3. **Worktree (`worktree.rs`)** — *deferred to v2*
   - First version: always create new worktree (current behavior). Dirty worktrees from previous run accumulate until manual cleanup.

4. **Storage**
   - `layout.json` in `<project>/.mosaic/` (created by `set_project` if missing)

### What MVP Explicitly Does NOT Do

- ❌ Reuse dirty worktrees (always fresh)
- ❌ Detect moved project (assume same path)
- ❌ Handle missing program gracefully (just shows error toast from spawn failure)
- ❌ Migrate `localStorage` layout settings (user re-sets columns/height once)

---

## Conductor Persistence (Critical Fix)

**Current bug:** Conductor role lives only in `Shared.conductor` (in-memory `Mutex<Option<String>>`). It is **not** in `brain.jsonl` and not in `localStorage`. App restart → conductor lost.

**Fix:** Conductor must be in `layout.json`. On `set_conductor` (Tauri command), also update `layout.json` immediately (debounced). On restore, `set_conductor` is called after panes spawn.

---

## Open Questions

1. **Should `layout.json` include terminal size (rows/cols)?** Currently `spawn_session` takes rows/cols from frontend xterm.js fit addon. Could persist per-pane but adds complexity. MVP: no.

2. **What about non-agent panes (Shell)?** They have `program: "powershell"` etc., `isolate: false`, no brain. Restore works identically — just spawns a shell.

3. **Should we version the brain assignment in `layout.json` to detect drift?** `brain.jsonl` is the source of truth for brain→markdown mapping. `layout.json` stores the *user's intent* (drag result). On conflict, layout wins. Simpler: no versioning.

4. **Cleanup of orphaned worktrees?** Accumulated dirty worktrees from abandoned restores need a "Clean up worktrees" action in Settings. Deferred.

---

## Implementation Order

1. Add `layout.json` read/write commands in `lib.rs`
2. Persist conductor to `layout.json` in `set_conductor`
3. Frontend: restore prompt on project select
4. Frontend: spawn panes from layout, call `set_conductor`
5. Frontend: auto-save layout on changes (debounced)
6. **v2:** Worktree reuse logic + dirty prompt
7. **v2:** Project-moved detection + program-missing UX

---

## Appendix: Current State Map

| Data | Current Location | Persists? | In Restore? |
|------|------------------|-----------|-------------|
| Pane list (id, type, brain, isolate) | React `useState` | ❌ | ✅ (from layout.json) |
| Layout (mode, columns, paneHeight) | `localStorage` | ✅ | ✅ (merge with layout.json) |
| Project dir | `localStorage` | ✅ | ✅ (trigger) |
| Conductor | `Shared.conductor` (memory) | ❌ | ✅ (from layout.json) |
| AgentSession (name, kind) | `brain.jsonl` | ✅ | ✅ (cross-ref) |
| Brain assignment (name→room) | `Shared.name_to_room` + `brain.jsonl` | ✅ | ✅ (from layout.json) |
| Worktree branch | Git repo + app data dir | ✅ | ✅ (from layout.json) |
| Worktree dirty state | Disk only | ✅ | ⚠️ (v2 prompt) |
| Tasks / Decisions / Facts | `brain.jsonl` | ✅ | ✅ (unaffected) |
| Scrollback / PTY | Process memory | ❌ | ❌ (lie) |