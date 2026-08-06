# Mosaic

A desktop cockpit for running several AI coding agents side by side. Each agent
gets a live terminal pane; panes can be grouped into a shared context store so
one agent's decision becomes another's knowledge without you relaying it by hand.

**Status:** working prototype. The session engine, shared context, worktree
isolation, and conductor all function, but see [Known gaps](#known-gaps) before
relying on it. Windows only — the terminal layer is ConPTY.

## What it does

- Runs Claude Code, Codex, opencode, or a plain PowerShell in parallel panes,
  each on its own pseudo-terminal.
- Connects agents to a **shared brain** — an in-process MCP server they use to
  record decisions and facts, broadcast, and read what the others have decided.
- Groups panes into **brains**: agents in the same brain share context, agents in
  different brains are isolated from each other. Drag a pane to re-home it.
- **Isolates** a session in its own git worktree and branch, so parallel agents
  editing one repo never clash.
- Promotes one pane to **conductor**, which can hand tasks to other sessions and
  collect their results.

## Quick start

Install `Mosaic_0.1.0_x64-setup.exe` and launch from the Start Menu, or run from
a checkout:

```powershell
pnpm install
.\dev.cmd        # sets up the MSVC environment, then `pnpm tauri dev`
```

To build the installer:

```powershell
.\build.cmd
```

Artifacts land in `src-tauri/target/release/`:

| Path | What it is |
|---|---|
| `mosaic.exe` | Standalone, no install needed |
| `bundle/nsis/Mosaic_0.1.0_x64-setup.exe` | Installer with a Start Menu entry |

Requires the Visual Studio 2022 Build Tools (both scripts call `vcvars64.bat`),
Rust, and pnpm.

## Usage

1. **Pick project** in the title bar — the git repo agents work in. It is
   remembered between runs, and the shared brain writes its notes to that
   project's `.mosaic/context/`.
2. **Ctrl+K** opens the launcher. Pick a session type. *Isolate* is on by
   default, giving that session its own worktree and branch.
3. **Drag a pane header** onto another pane, or onto a brain in the sidebar, to
   put them in the same brain.
4. **⌁ on a pane** promotes it to conductor. Dispatched tasks are typed into the
   target's visible terminal, so you see every instruction. **Stop** halts all
   dispatch immediately.
5. **Layout: scroll** keeps every terminal at a comfortable minimum height and
   scrolls the cockpit. Open **Layout** to choose automatic or fixed 1–4 column
   arrangements and a minimum pane height, or switch to **Fit window** when you
   want every pane visible at once. **Maximize** expands one pane without stopping
   or remounting the others; use **Restore** to return to the grid.
6. **Ctrl+Shift+B** toggles the sidebar; **Ctrl+Shift+,** opens appearance
   settings; **Ctrl+Shift+K** opens the session launcher. The Shift modifier
   keeps common terminal controls such as Ctrl+B and Ctrl+K available to agents.
   **Ctrl+Shift+1…9** focuses a session; **Ctrl+Shift+Enter** maximizes or restores
   the active terminal.

## How agents connect

Every session gets its own MCP endpoint on a random loopback port, wired in at
launch through arguments and environment only:

| Session | Mechanism |
|---|---|
| Claude Code | `--mcp-config <per-session file>` — additive, your other MCP servers still load |
| Codex | `-c mcp_servers.mosaic.url=…` |
| opencode | `OPENCODE_CONFIG=<per-session file>` — merged over your global config |
| Shell | none |

Mosaic never writes to your global agent config. Because a port is only ever
handed to one session, Mosaic knows which agent is calling from the connection
alone — the agent never declares a name and cannot claim another's.

Agents get these tools: `record_decision`, `record_fact`, `broadcast`,
`get_shared_context`, `search_context`, `list_sessions`, `dispatch` (conductor
only), `complete_task`, `get_task_result`, `set_session_identity`.

## Guardrails

- Only the conductor can dispatch, and the app assigns that role — an agent
  cannot claim it. Depth is bounded structurally: a dispatched agent is not the
  conductor, so it cannot dispatch onward.
- 40 dispatches per run, 10-minute task timeout, and a Stop that cancels
  everything pending.
- A worktree branch is deleted only when it has no commits of its own, so agent
  work is never silently discarded.

## Project structure

| Path | Contents |
|---|---|
| `src/` | React UI — panes, brains sidebar, conductor bar, theming |
| `src-tauri/src/lib.rs` | PTY session engine and Tauri commands |
| `src-tauri/src/mcp.rs` | The shared brain: MCP server, context store, dispatch |
| `src-tauri/src/worktree.rs` | Git worktree isolation |
| `ui-gallery/` | Standalone design explorations, not part of the build |

## Known gaps

- **Agents are not yet told to use the brain.** The tools are exposed and
  described, but nothing instructs an agent to read shared context before
  deciding or record decisions after. Until that exists — a skill, or a line in
  the project's `AGENTS.md` — the brain stays mostly empty in practice.
- **Isolated work has no merge path.** A session's branch (`mosaic/<id>-<uid>`)
  survives when it has commits, but the UI never shows the branch name or a diff.
- **Dispatch assumes an idle target.** Instructions are typed into the target's
  terminal; if that agent is mid-task the input is swallowed and the task sits
  pending until it times out.
- Fit layout is intended for at most 6 panes; scroll layout remains usable beyond that.
- No automated tests.
