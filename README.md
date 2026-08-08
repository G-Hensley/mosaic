# Mosaic

**A desktop cockpit for running several AI coding agents side by side, coordinated instead of siloed.**

Each agent gets a live terminal pane. Panes can be grouped into a shared
context store so one agent's decision becomes another's knowledge without a
person relaying it by hand, and any pane can be promoted to conductor to fan
work out to the rest and collect the results.

**Status:** working prototype. The session engine, shared context, worktree
isolation, and conductor all function, but see [Known gaps and limitations](#known-gaps-and-limitations)
before relying on it. Windows only: the terminal layer is ConPTY.

## Why it exists

Running Claude Code, Codex, and opencode in parallel panes is easy. Getting
them to act like a team instead of three strangers duplicating each other's
work is the actual problem: nothing tells one agent what another just
decided, and nothing lets a person hand out tasks without babysitting every
pane. Mosaic answers both. A shared MCP server lets agents record decisions
and read each other's, git worktree isolation lets them edit the same repo
without clashing, and a conductor role lets one pane fan tasks out to the
others and collect what comes back.

## Capabilities

- Runs Claude Code, Codex, opencode, or a plain PowerShell in parallel panes,
  each on its own pseudo-terminal.
- Connects agents to a **shared brain**, an in-process MCP server they use to
  record decisions and facts, broadcast, and read what the others have
  decided.
- Groups panes into **brains**: agents in the same brain share context, agents
  in different brains are isolated from each other. Drag a pane to re-home it.
- **Isolates** a session in its own git worktree and branch, so parallel
  agents editing one repo never clash.
- Promotes one pane to **conductor**, which can hand tasks to other sessions
  and collect their results.

## 60-second demo

1. Open Mosaic and pick the git repo you want agents working in.
2. Press **Ctrl+Shift+K** and launch two or three sessions (a mix of Claude
   Code, Codex, or opencode). Isolate is on by default, so each gets its own
   worktree and branch.
3. Drag pane headers together to put them in the same brain. They can now see
   each other's recorded decisions.
4. Click **⌁** on one pane to promote it to conductor, then type a task and
   press Enter. The conductor dispatches work to the other panes by typing
   into their visible terminals, so every instruction is something you can
   watch happen.
5. Watch the conductor bar for task pills as work completes, and check the
   context sidebar for the decisions agents recorded along the way.

## Quick start

Install `Mosaic_0.1.0_x64-setup.exe` and launch from the Start Menu, or run
from a checkout:

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
Rust, and pnpm. See [CONTRIBUTING.md](CONTRIBUTING.md) for the full setup and
test workflow.

## Usage

1. **Pick project** in the title bar: the git repo agents work in. It is
   remembered between runs, and the shared brain writes its notes to that
   project's `.mosaic/context/`.
2. **Ctrl+Shift+K** opens the launcher. Pick a session type. *Isolate* is on
   by default, giving that session its own worktree and branch.
3. **Drag a pane header** onto another pane, or onto a brain in the sidebar,
   to put them in the same brain.
4. **⌁ on a pane** promotes it to conductor. Dispatched tasks are typed into
   the target's visible terminal, so you see every instruction. **Stop** halts
   all dispatch immediately.
5. **Layout: scroll** keeps every terminal at a comfortable minimum height and
   scrolls the cockpit. Open **Layout** to choose automatic or fixed 1 to 6
   column arrangements and a minimum pane height, or switch to **Fit window**
   when you want every pane visible at once. **Maximize** expands one pane
   without stopping or remounting the others; use **Restore** to return to the
   grid.
6. **Ctrl+Shift+B** toggles the sidebar; **Ctrl+Shift+,** opens appearance
   settings; **Ctrl+Shift+K** opens the session launcher. The Shift modifier
   keeps common terminal controls such as Ctrl+B and Ctrl+K available to
   agents. **Ctrl+Shift+1…9** focuses a session; **Ctrl+Shift+Enter** maximizes
   or restores the active terminal.

## How agents connect

Every session gets its own MCP endpoint on a random loopback port, wired in at
launch through arguments and environment only:

| Session | Mechanism |
|---|---|
| Claude Code | `--mcp-config <per-session file>` (additive; your other MCP servers still load) |
| Codex | `-c mcp_servers.mosaic.url=…` |
| opencode | `OPENCODE_CONFIG=<per-session file>` (merged over your global config) |
| Shell | none |

Mosaic never writes to your global agent config. Because a port is only ever
handed to one session, Mosaic knows which agent is calling from the connection
alone: the agent never declares a name and cannot claim another's.

Agents get these tools: `record_decision`, `record_fact`, `broadcast`,
`get_shared_context`, `search_context`, `list_sessions`, `dispatch` (conductor
only), `complete_task`, `get_task_result`, `set_session_identity`.

## How agents are briefed

Tools alone don't change behaviour. An agent that isn't told the other panes
are usable capacity will quietly do everything itself. Mosaic briefs agents on
two channels, because they answer different questions:

| Channel | Delivered | Says |
|---|---|---|
| MCP server instructions | Once, on connect | You are in Mosaic, here is the shared brain, here is what the conductor role means if you're given it |
| Composer prefill | On promotion to conductor | You are the conductor *now*, here are the live sessions by name and model |

The prefill is typed into the pane's input but **not sent**. Add your first
instruction after it and press Enter, and the agent gets its role and its task
together. Nothing is dispatched or spent until you do. It stays short for that
reason; the detail lives in the connect-time instructions instead.

All three agent CLIs surface the connect-time instructions to their model,
verified by asking a live Claude Code, Codex, and opencode session to quote
the first sentence back.

The second exists because MCP hands a client its instructions once, at connect
time, while the conductor is chosen by the user later and can change mid-run.
An agent promoted at minute ten has never been told it now commands the
workspace, so the role change is typed into its terminal the moment it
happens; it is the only channel that reaches an already-running agent.

Dispatch returns a `task_id` immediately rather than blocking, so a conductor
is meant to fan several tasks out and then collect them; `get_task_result`
with no `task_id` returns every task that conductor dispatched in one call.

## Guardrails

- Only the conductor can dispatch, and the app assigns that role. An agent
  cannot claim it. Depth is bounded structurally: a dispatched agent is not
  the conductor, so it cannot dispatch onward.
- 40 dispatches per run, 10-minute task timeout, and a Stop that cancels
  everything pending.
- A worktree branch is deleted only when it has no commits of its own, so
  committed agent work is never silently discarded. Cleanup also refuses to
  remove a dirty worktree, preserving uncommitted changes on disk.

## Project structure

| Path | Contents |
|---|---|
| `src/` | React UI: panes, brains sidebar, conductor bar, theming |
| `src-tauri/src/lib.rs` | PTY session engine and Tauri commands |
| `src-tauri/src/mcp.rs` | The shared brain: MCP server, context store, dispatch |
| `src-tauri/src/worktree.rs` | Git worktree isolation |
| `ui-gallery/` | Standalone design explorations, not part of the build |

## Known gaps and limitations

- **Isolated work has no merge path.** A session's branch (`mosaic/<id>-<uid>`)
  survives when it has commits, but the UI never shows the branch name or a
  diff.
- **Dispatch assumes an idle target.** Instructions are typed into the
  target's terminal; if that agent is mid-task the input is swallowed and the
  task sits pending until it times out.
- **Dirty worktree recovery is manual.** Closing a session refuses to remove
  its dirty worktree, preserving uncommitted edits on disk, but the UI does
  not yet show the preserved path or offer a recovery workflow.
- **Shared context does not persist across restarts.** It lives in memory for
  the running session; the markdown mirror under `.mosaic/context/` is
  written for humans to read, not read back in on launch.
- Fit layout is intended for at most 6 panes; scroll layout remains usable
  beyond that.
- Thin test coverage: `cargo test` covers PTY submit timing and shared-brain
  prompt formatting. Worktree isolation and the whole frontend are untested.
- Single-user, single-machine threat model. See [SECURITY.md](SECURITY.md)
  for what that means in practice before pointing Mosaic at anything
  sensitive.

## Verification

```powershell
cd src-tauri
cargo test        # Rust unit tests: PTY submit timing, shared-brain formatting
cd ..
pnpm build         # tsc in strict mode, then the Vite production build
```

CI runs both on every pull request (see [CONTRIBUTING.md](CONTRIBUTING.md)),
along with `cargo fmt --check`, `cargo clippy`, and dependency audits. Run them
locally first so review starts from a green branch.

## Contributing and security

Contributions are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md) for the
build, test, and branching workflow. For vulnerability reports, see
[SECURITY.md](SECURITY.md) rather than opening a public issue.

## License

[MIT](LICENSE)
