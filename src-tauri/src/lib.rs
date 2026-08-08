// Mosaic — multi-session PTY engine.
//
// Each live agent runs on its own pseudo-terminal (ConPTY). Sessions are keyed
// by a client-supplied id so the frontend can address a pane (write/resize/kill)
// the moment it asks to spawn it, while the streaming command keeps running in
// the background. In later milestones this grows a TOML registry + git-worktree
// isolation; here it's the flat, working core.

mod mcp;
mod worktree;

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

/// One live PTY's non-reader handles (the reader is moved into its own thread).
struct SessionHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Present when this session runs isolated in its own git worktree.
    worktree: Option<worktree::Worktree>,
    /// When this session last produced output, on the `mono_ms` clock. Written by
    /// the reader thread, read by `submit_to` to tell when a target has finished
    /// absorbing a pasted prompt. See `SUBMIT_QUIET_MS`.
    last_output: Arc<AtomicU64>,
    /// The program this session launched, so `submit_to` can tell which CLI it is
    /// typing into. Only Codex gets bracketed-paste framing; see `PASTE_START`.
    program: String,
}

/// Process-monotonic millisecond clock, used to time the gap between a prompt
/// and the Enter that submits it.
///
/// Deliberately not `SystemTime`: that can step backwards (NTP correction, a DST
/// change), which would make a target's last-output timestamp look arbitrarily
/// far in the past. Quiet would then read as satisfied instantly and Enter would
/// fire early — precisely the failure this module exists to prevent.
static CLOCK: LazyLock<Instant> = LazyLock::new(Instant::now);

fn mono_ms() -> u64 {
    CLOCK.elapsed().as_millis() as u64
}

/// How long a target's output must be quiet before we accept that it has
/// finished receiving the prompt.
///
/// Sized against documented behaviour in the Codex TUI. Codex infers a paste
/// from keystroke burst timing, and suppresses Enter for 120 ms afterwards —
/// but it re-anchors that window on *every* buffered character, so the deadline
/// keeps moving for as long as bytes are still arriving. An Enter that lands
/// inside the window is absorbed into the composer as a newline, and the agent
/// never sees the task: no error, no timeout signal, just a pane sitting idle
/// with the instruction visible but unsent.
///
/// The previous fixed sleep measured from our own `write_all` returning, which
/// is the wrong anchor — that only means the bytes reached the PTY buffer, not
/// that the target consumed them. For a large payload ConPTY is still delivering
/// well after the call returns, so a 1946-character dispatch reliably lost its
/// Enter. Waiting for the target's *output* to stop instead measures the thing
/// that actually matters, and adapts to payload size and machine load for free.
/// 200 ms clears Codex's 120 ms window with room for delivery lag.
const SUBMIT_QUIET_MS: u64 = 200;

/// Never send Enter sooner than this after the write, however quiet the target
/// looks. A CLI that renders nothing in response to input is "quiet" the instant
/// we finish writing, which tells us nothing at all — this floor is what covers
/// that case. It is also the delay the previous implementation used, so short
/// prompts wait exactly as long as they did before and cannot regress.
const SUBMIT_FLOOR_MS: u64 = 300;

/// Give up waiting for quiet and send Enter regardless. An agent that is already
/// mid-task streams output continuously and would never look quiet, so without a
/// ceiling its dispatch would wait forever. This bounds the wait instead.
const SUBMIT_CEILING_MS: u64 = 4000;

/// How often to re-check for quiet while waiting.
const SUBMIT_POLL_MS: u64 = 20;

/// Bracketed paste markers (DECSET 2004). Wrapping a payload in these makes it
/// one explicit paste event with a defined end, instead of something the target
/// has to infer from keystroke burst timing — which removes the guesswork the
/// timing policy above can only approximate.
///
/// Applied to Codex alone, deliberately. Codex is the CLI whose burst inference
/// loses the Enter, and it recommends this framing for itself. Claude Code
/// already submits reliably, so there is nothing to gain there and a real regression
/// to risk if it turned out not to honour the markers: an unsupported sequence
/// does not vanish, it lands in the composer as literal junk. opencode is
/// verified to support them and can be added once it has been exercised.
const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

fn is_codex(program: &str) -> bool {
    let p = program.to_ascii_lowercase();
    p.trim_end_matches(".exe").trim_end_matches(".cmd") == "codex"
}

/// Whether the Enter that submits a prompt can be sent yet.
///
/// Split out from the waiting loop so the policy is testable without a live PTY.
///
/// `baseline` is the target's output clock sampled *before* the prompt was
/// written, and comparing against it is what makes this correct. The previous
/// version asked only "has output been quiet for a while", which a target that
/// stays silent while it buffers a paste satisfies trivially — its last output
/// predates the write, so the gap is already enormous and Enter fires the moment
/// the floor elapses. That is exactly the original bug wearing a new hat, and it
/// is what a large dispatch to Codex hit: it echoes nothing until its burst
/// flushes, so "quiet" meant "has not started yet" rather than "has finished".
///
/// Silence before the target has produced anything therefore counts as still
/// receiving. Only once it has actually said something does going quiet mean it
/// is done, and the ceiling still bounds the wait if it never speaks at all.
fn ready_to_submit(now: u64, started: u64, last_output: u64, baseline: u64) -> bool {
    let waited = now.saturating_sub(started);
    // Checked first so a silent target is always released eventually.
    if waited >= SUBMIT_CEILING_MS {
        return true;
    }
    if waited < SUBMIT_FLOOR_MS {
        return false;
    }
    if last_output == baseline {
        return false;
    }
    now.saturating_sub(last_output) >= SUBMIT_QUIET_MS
}

#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionHandle>>,
    /// Serialize ConPTY openpty+spawn: concurrent spawns can stall a PTY pipe on
    /// Windows, so only one session is created at a time.
    spawn_lock: Mutex<()>,
}

fn report_worktree_cleanup(worktree: &worktree::Worktree) {
    match worktree::remove(worktree) {
        Ok(worktree::RemoveOutcome::RefusedDirty) => eprintln!(
            "[mosaic] preserved dirty worktree at {}",
            worktree.path.display()
        ),
        Ok(_) => {}
        Err(error) => eprintln!(
            "[mosaic] worktree cleanup failed for {}: {error}",
            worktree.path.display()
        ),
    }
}

impl SessionManager {
    fn kill(&self, id: &str) {
        if let Some(mut h) = self.sessions.lock().unwrap().remove(id) {
            let _ = h.child.kill();
            if let Some(wt) = &h.worktree {
                report_worktree_cleanup(wt);
            }
            let _ = std::fs::remove_dir_all(session_config_dir(id));
        }
    }

    /// Type text into a session's terminal. This is how the conductor dispatches
    /// work: the task lands visibly in the target agent's pane.
    pub fn write_to(&self, id: &str, data: &str) -> bool {
        let mut map = self.sessions.lock().unwrap();
        if let Some(h) = map.get_mut(id) {
            if h.writer.write_all(data.as_bytes()).is_ok() {
                let _ = h.writer.flush();
                return true;
            }
        }
        false
    }

    /// The session's output-activity clock and launched program, if it is live.
    fn session_meta(&self, id: &str) -> Option<(Arc<AtomicU64>, String)> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|h| (h.last_output.clone(), h.program.clone()))
    }

    /// Submit a prompt to a terminal UI as typing followed by a distinct Enter
    /// keypress. Codex and Claude Code deliberately distinguish pasted text
    /// containing a carriage return from an interactive Enter event; writing
    /// both in one PTY operation can leave the prompt sitting in the editor.
    ///
    /// The prompt is written synchronously, so an unreachable session is still
    /// reported to the caller. The Enter is not: it waits for the target to stop
    /// producing output (see `SUBMIT_QUIET_MS`) on a thread of its own, because
    /// that wait is open-ended and dispatch is specifically documented to return
    /// immediately so a conductor can fan work out. Blocking here would serialize
    /// a fan-out into one round trip per target, which is the property dispatch
    /// exists to provide.
    ///
    /// A `true` return therefore means "delivered to the terminal", not "already
    /// submitted".
    pub fn submit_to(self: &Arc<Self>, id: &str, prompt: &str) -> bool {
        // Resolved before the write so the output clock can be sampled first:
        // the baseline is only meaningful if it predates the prompt.
        let Some((clock, program)) = self.session_meta(id) else {
            return false;
        };
        let baseline = clock.load(Ordering::Relaxed);

        let payload: Cow<'_, str> = if is_codex(&program) {
            Cow::Owned(format!("{PASTE_START}{prompt}{PASTE_END}"))
        } else {
            Cow::Borrowed(prompt)
        };
        if !self.write_to(id, &payload) {
            return false;
        }

        let engine = self.clone();
        let id = id.to_string();
        thread::spawn(move || {
            let started = mono_ms();
            while !ready_to_submit(mono_ms(), started, clock.load(Ordering::Relaxed), baseline) {
                thread::sleep(Duration::from_millis(SUBMIT_POLL_MS));
            }
            engine.write_to(&id, "\r");
        });
        true
    }

    /// Ids of every live session.
    pub fn ids(&self) -> Vec<String> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }

    /// The program a live session launched, if it's still live — lets a
    /// caller (here, `human_dispatch`) tell an agent CLI target from a plain
    /// shell without going through the MCP-side session list, which may not
    /// have an entry yet if the session's dedicated endpoint failed to bind
    /// (see the fallback path in `spawn_session`).
    pub fn program_of(&self, id: &str) -> Option<String> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|h| h.program.clone())
    }

    /// Kill every session — used on window close so no agent process is orphaned
    /// and no worktree is left behind.
    fn kill_all(&self) {
        let mut map = self.sessions.lock().unwrap();
        for (id, mut h) in map.drain() {
            let _ = h.child.kill();
            if let Some(wt) = &h.worktree {
                report_worktree_cleanup(wt);
            }
            let _ = std::fs::remove_dir_all(session_config_dir(&id));
        }
    }
}

/// Decide how to launch `program` (see the M1 note): real shells launch directly;
/// everything else (claude/codex/opencode `.cmd` shims) is routed through
/// `cmd.exe /c <bare-name>` so PATH — not our quoting — resolves it.
fn build_command(
    program: &str,
    args: &[String],
    cwd: &Path,
    extra_env: &[(String, String)],
) -> CommandBuilder {
    let is_native_shell = matches!(
        program.to_ascii_lowercase().trim_end_matches(".exe"),
        "powershell" | "pwsh" | "cmd" | "bash" | "wsl"
    );

    let mut cmd = if is_native_shell {
        let mut c = CommandBuilder::new(program);
        for a in args {
            c.arg(a);
        }
        c
    } else {
        let mut c = CommandBuilder::new("cmd.exe");
        c.arg("/c");
        c.arg(program);
        for a in args {
            c.arg(a);
        }
        c
    };

    // Launch in the session's directory — the project repo, or its own worktree
    // when isolated. This also decides which git root the agent resolves to,
    // which is what Claude keys its local-scope MCP registration by.
    cmd.cwd(cwd);

    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    // Applied after the inherited environment so per-session wiring wins.
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd
}

/// Root for everything Mosaic stores per machine: worktrees, per-session agent
/// config, and the fallback context dir.
///
/// Keyed by bundle identifier rather than product name on purpose. A per-user
/// install puts the app itself in `%LOCALAPPDATA%\Mosaic`, and Windows paths are
/// case-insensitive — so a plain "mosaic" here would mix runtime data in with
/// the installed binaries, and an uninstall could take an agent's worktree with it.
pub fn app_data_dir() -> PathBuf {
    let root = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
    PathBuf::from(root).join("com.gavinhensley.mosaic")
}

/// Per-session scratch dir for the config files we hand an agent CLI at launch.
fn session_config_dir(session_id: &str) -> PathBuf {
    app_data_dir().join("sessions").join(session_id)
}

/// Whether a session's program is an agent CLI that gets wired to the shared
/// brain. Kept in step with the match in `agent_mcp_wiring`.
///
/// A Shell pane can be promoted to conductor like any other, but it has no MCP
/// connection and so can never dispatch. Typing a briefing into it would just
/// hand PowerShell a paragraph of prose to run.
pub fn is_agent_cli(program: &str) -> bool {
    let p = program.to_ascii_lowercase();
    let p = p.trim_end_matches(".exe").trim_end_matches(".cmd");
    matches!(p, "claude" | "codex" | "opencode")
}

/// Trim and validate the `human_dispatch` inputs, so the rule is the same
/// whether the emptiness comes from the UI or a caller invoking the command
/// directly. Pure and separate from the Tauri command so it's testable
/// without constructing app state.
fn validate_human_dispatch(target: &str, task: &str) -> Result<(String, String), String> {
    let target = target.trim();
    let task = task.trim();
    if target.is_empty() {
        return Err("target is required.".to_string());
    }
    if task.is_empty() {
        return Err("task text is required.".to_string());
    }
    Ok((target.to_string(), task.to_string()))
}

/// Point ONE agent CLI at ONE dedicated MCP endpoint, entirely through launch
/// arguments and environment — we never touch the user's global config.
///
/// That matters twice over. It keeps identity honest (a session's port is only
/// ever registered to that session, so Mosaic knows the caller from the
/// connection alone), and it leaves no stale `mosaic` server behind pointing at
/// a random port that died with the app.
///
/// Config is written to a file rather than passed inline because these commands
/// are routed through `cmd.exe /c` — a JSON blob on that command line would be
/// at the mercy of Windows quoting.
///
/// Returns `(extra_args, extra_env)` to fold into the launch.
fn agent_mcp_wiring(
    program: &str,
    session_id: &str,
    url: &str,
) -> (Vec<String>, Vec<(String, String)>) {
    let prog = program.to_ascii_lowercase();
    let prog = prog.trim_end_matches(".exe").trim_end_matches(".cmd");
    let dir = session_config_dir(session_id);
    let _ = std::fs::create_dir_all(&dir);

    match prog {
        // Additive on purpose: `--strict-mcp-config` would suppress every other
        // MCP server the user has configured, so a Mosaic pane would silently
        // lose the rest of their toolkit.
        //
        // `--mcp-config` is VARIADIC — it keeps eating following arguments as
        // further config paths. These are appended last for that reason. Any
        // argument added after this one would be swallowed as a config file.
        "claude" => {
            let path = dir.join("claude-mcp.json");
            let body = format!(r#"{{"mcpServers":{{"mosaic":{{"type":"http","url":"{url}"}}}}}}"#);
            match std::fs::write(&path, body) {
                Ok(_) => (
                    vec![
                        "--mcp-config".to_string(),
                        path.to_string_lossy().to_string(),
                    ],
                    vec![],
                ),
                Err(e) => {
                    eprintln!("[mosaic] claude mcp config write failed: {e}");
                    (vec![], vec![])
                }
            }
        }
        // A bare value fails TOML parsing, at which point Codex documents that it
        // falls back to the raw string — which sidesteps nested quoting.
        "codex" => (
            vec!["-c".to_string(), format!("mcp_servers.mosaic.url={url}")],
            vec![],
        ),
        // OPENCODE_CONFIG is merged over the global config, not swapped for it.
        "opencode" => {
            let path = dir.join("opencode.json");
            let body = format!(
                r#"{{"$schema":"https://opencode.ai/config.json","mcp":{{"mosaic":{{"type":"remote","url":"{url}"}}}}}}"#
            );
            match std::fs::write(&path, body) {
                Ok(_) => (
                    vec![],
                    vec![(
                        "OPENCODE_CONFIG".to_string(),
                        path.to_string_lossy().to_string(),
                    )],
                ),
                Err(e) => {
                    eprintln!("[mosaic] opencode config write failed: {e}");
                    (vec![], vec![])
                }
            }
        }
        // Plain shells get no wiring — nothing to connect.
        _ => (vec![], vec![]),
    }
}

/// Spawn a session under `session_id` and stream its output over `channel` until
/// the child exits. Returns as soon as streaming ends; the frontend fires it
/// without awaiting and addresses the session by the id it supplied.
#[tauri::command]
async fn spawn_session(
    app: AppHandle,
    state: State<'_, Arc<SessionManager>>,
    mcp: State<'_, McpInfo>,
    shared: State<'_, Arc<mcp::Shared>>,
    session_id: String,
    channel: Channel<&[u8]>,
    program: String,
    args: Vec<String>,
    rows: u16,
    cols: u16,
    cwd: Option<String>,
    isolate: Option<bool>,
) -> Result<(), String> {
    // Decide where this session runs: the project dir, or its own git worktree
    // when isolated. Done before taking the spawn lock — creating a worktree and
    // registering MCP takes seconds and shouldn't serialize other spawns.
    let project = cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let mut session_cwd = project.clone();
    let mut wt: Option<worktree::Worktree> = None;
    if isolate.unwrap_or(false) {
        if let Some(root) = worktree::repo_root(&project) {
            match worktree::create(&root, &session_id) {
                Ok(w) => {
                    session_cwd = w.path.clone();
                    wt = Some(w);
                }
                // Fall back to the project dir rather than failing the launch.
                Err(e) => eprintln!("[mosaic] worktree create failed: {e}"),
            }
        }
    }

    // EVERY session gets its own endpoint, isolated or not. Because that port is
    // only ever handed to this one session, any request arriving on it is
    // provably from it — so identity needs no handshake and can't be spoofed or
    // forgotten. Sessions sharing one endpoint would all authenticate as
    // "unknown", which silently breaks brain assignment and the conductor.
    let (extra_args, extra_env) =
        match mcp::start_session_server(shared.inner().clone(), session_id.clone()) {
            Ok(p) => {
                shared.note_session(&session_id, &program);
                agent_mcp_wiring(&program, &session_id, &format!("http://127.0.0.1:{p}/mcp"))
            }
            // Endpoint failed: fall back to the shared one. The agent can still reach
            // the brain, it just has to declare who it is.
            Err(e) => {
                eprintln!("[mosaic] session endpoint failed, using shared: {e}");
                agent_mcp_wiring(&program, &session_id, &mcp.url)
            }
        };
    let mut args = args;
    args.extend(extra_args);

    // Starts "active now" so a session that never writes anything is governed by
    // SUBMIT_FLOOR_MS rather than looking quiet since the epoch.
    let activity = Arc::new(AtomicU64::new(mono_ms()));

    // Create the PTY + child under the spawn lock (serialize ConPTY spawns). The
    // lock guard is confined to this block so it never crosses the .await below.
    let mut reader = {
        let _guard = state.spawn_lock.lock().unwrap();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = build_command(&program, &args, &session_cwd, &extra_env);
        // The agent's Mosaic name = its session id, so the collab skill can
        // self-identify and the app can map it to a brain.
        cmd.env("MOSAIC_SESSION", &session_id);
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave); // so the reader hits EOF when the child exits

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        state.sessions.lock().unwrap().insert(
            session_id.clone(),
            SessionHandle {
                master: pair.master,
                writer,
                child,
                worktree: wt,
                last_output: activity.clone(),
                program: program.clone(),
            },
        );
        reader
    };

    // Blocking reads on their own thread → async forward loop via mpsc.
    // Bound each session's pending output to roughly 512 KiB. Backpressure here
    // is preferable to six unbounded queues consuming memory when WebView2 is
    // busy or minimized; ConPTY naturally blocks the child until we catch up.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Stamped on arrival rather than after the send, which can
                    // block on a full channel and would misreport the target as
                    // quiet while it is in fact still talking.
                    activity.store(mono_ms(), Ordering::Relaxed);
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    while let Some(mut bytes) = rx.recv().await {
        // Drain queued bursts into fewer IPC messages without adding latency to
        // interactive output. Keep batches modest so one noisy agent cannot
        // monopolize the WebView event loop.
        while bytes.len() < 64 * 1024 {
            match rx.try_recv() {
                Ok(next) => bytes.extend_from_slice(&next),
                Err(_) => break,
            }
        }
        if channel.send(&bytes[..]).is_err() {
            break;
        }
    }

    // Session ended on its own (agent quit or crashed). Tear down exactly what
    // an explicit kill would: dropping the handle alone would discard the
    // Worktree without removing it, stranding a directory and a branch on disk
    // for every session that wasn't closed by hand.
    let handle = state.sessions.lock().unwrap().remove(&session_id);
    if let Some(h) = handle {
        if let Some(w) = &h.worktree {
            report_worktree_cleanup(w);
        }
    }
    let _ = std::fs::remove_dir_all(session_config_dir(&session_id));
    let _ = app.emit("session-exited", &session_id);
    Ok(())
}

#[tauri::command]
fn write_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let mut map = state.sessions.lock().unwrap();
    if let Some(h) = map.get_mut(&session_id) {
        h.writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        h.writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn resize_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let map = state.sessions.lock().unwrap();
    if let Some(h) = map.get(&session_id) {
        h.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn kill_session(state: State<'_, Arc<SessionManager>>, session_id: String) {
    state.kill(&session_id);
}

/// Where the shared brain writes its markdown before a project is chosen.
/// App-local, so it is never at the mercy of the launch directory.
fn default_context_dir() -> PathBuf {
    app_data_dir().join("context")
}

/// Point the shared brain's markdown at the project the user picked, so a
/// project's decisions live with that project instead of in a global pile.
/// Called by the frontend on startup (from the remembered project) and on pick.
#[tauri::command]
fn set_project(shared: State<'_, Arc<mcp::Shared>>, path: Option<String>) {
    let dir = match path {
        Some(p) if !p.is_empty() => PathBuf::from(p).join(".mosaic").join("context"),
        _ => default_context_dir(),
    };
    shared.set_dir(dir);
}

/// Loopback URL + port of the in-process MCP server, for per-session registration.
#[derive(Clone, Serialize)]
struct McpInfo {
    url: String,
    port: u16,
}

#[tauri::command]
fn mcp_info(info: State<'_, McpInfo>) -> McpInfo {
    info.inner().clone()
}

/// A snapshot of the shared brain for the sidebar.
#[derive(Serialize)]
struct ContextSnapshot {
    entries: Vec<mcp::Entry>,
    sessions: Vec<mcp::AgentSession>,
}

#[tauri::command]
fn get_context(shared: State<'_, Arc<mcp::Shared>>) -> ContextSnapshot {
    ContextSnapshot {
        entries: shared.entries_snapshot(),
        sessions: shared.sessions_snapshot(),
    }
}

/// Assign an agent (by the name it declared) to a brain. The frontend calls this
/// when a pane is created and whenever you drag it into a different brain.
#[tauri::command]
fn set_agent_brain(shared: State<'_, Arc<mcp::Shared>>, name: String, brain: String) {
    shared.set_room(&name, &brain);
}

/// Conductor role + halt state + the task feed, for the ConductorBar.
#[derive(Serialize)]
struct ConductorState {
    conductor: Option<String>,
    halted: bool,
    tasks: Vec<mcp::Task>,
}

#[tauri::command]
fn conductor_state(shared: State<'_, Arc<mcp::Shared>>) -> ConductorState {
    ConductorState {
        conductor: shared.conductor(),
        halted: shared.is_halted(),
        tasks: shared.tasks_snapshot(),
    }
}

/// Promote a pane to conductor (or pass null to clear). The app owns this role —
/// an agent can never claim it for itself.
#[tauri::command]
fn set_conductor(shared: State<'_, Arc<mcp::Shared>>, name: Option<String>) {
    shared.set_conductor(name);
}

/// The global Stop: halts all dispatch and cancels pending tasks.
#[tauri::command]
fn halt_conductor(shared: State<'_, Arc<mcp::Shared>>, halted: bool) {
    shared.set_halted(halted);
}

/// Let a human dispatch a task to a live agent session directly from the UI,
/// instead of only through an agent calling the MCP `dispatch` tool. Reuses
/// the exact submit machinery and task store that tool uses — see
/// `mcp::Shared::dispatch_task` — so the ConductorBar and get_task_result see
/// one ledger regardless of which path started the task.
///
/// Attributed to the current conductor rather than to some "user" identity:
/// the app is what decides who holds that role, and a task the human triggers
/// through the UI is still work the conductor is orchestrating, so it belongs
/// on the same ledger the conductor's own dispatches land on.
#[tauri::command]
fn human_dispatch(
    state: State<'_, Arc<SessionManager>>,
    shared: State<'_, Arc<mcp::Shared>>,
    target: String,
    task: String,
) -> Result<mcp::DispatchOutcome, String> {
    let (target, task) = validate_human_dispatch(&target, &task)?;

    let from = shared
        .conductor()
        .ok_or_else(|| "no conductor is set — promote a pane first.".to_string())?;

    // Only a live agent CLI can act on a dispatch: it needs the MCP
    // connection to read the task and call complete_task back. A Shell pane
    // has neither, so a dispatch to one would just sit pending until it
    // timed out.
    match state.program_of(&target) {
        Some(p) if is_agent_cli(&p) => {}
        Some(_) => {
            return Err(format!(
                "'{target}' is a shell session and cannot receive a dispatch."
            ))
        }
        None => return Err(format!("no live session '{target}'.")),
    }

    shared.dispatch_task(&from, &target, &task)
}

#[cfg(test)]
mod tests {
    use super::{
        is_agent_cli, is_codex, ready_to_submit, validate_human_dispatch, SUBMIT_CEILING_MS,
        SUBMIT_FLOOR_MS, SUBMIT_QUIET_MS,
    };

    // A dispatch is only lost in one direction: an Enter sent too early is
    // swallowed silently, while one sent late costs nothing but a pause. Every
    // case below is therefore written from the "hold unless certain" side.
    //
    // `baseline` is the output clock as of the write. Cases where it still equals
    // `last_output` are the target not having reacted yet.

    // Stands in for the output clock as of the write. Kept below the `now` values
    // used here so the cases stay arithmetically honest.
    const BASE: u64 = 100;

    #[test]
    fn holds_below_the_floor() {
        assert!(!ready_to_submit(SUBMIT_FLOOR_MS - 1, 0, BASE + 1, BASE));
    }

    // The regression that shipped: a target which stays silent while it buffers a
    // paste has a last-output timestamp predating the write, so "quiet for long
    // enough" was true the instant the floor elapsed and Enter fired straight into
    // the paste. Silence before the target has said anything must not count.
    #[test]
    fn holds_while_a_silent_target_has_not_reacted_to_the_write_yet() {
        // Inside the ceiling window, so this isolates the has-it-reacted rule
        // rather than the backstop that eventually overrides it.
        let now = SUBMIT_FLOOR_MS + 500;
        assert!(now < SUBMIT_CEILING_MS);
        // Stale by design: the target has produced nothing since before the write,
        // so an elapsed-quiet check alone would read this as "finished" and fire.
        assert!(now - BASE >= SUBMIT_QUIET_MS);
        assert!(!ready_to_submit(now, 0, BASE, BASE));
    }

    #[test]
    fn submits_once_the_target_has_spoken_and_then_gone_quiet() {
        let now = SUBMIT_FLOOR_MS + SUBMIT_QUIET_MS;
        let last_output = now - SUBMIT_QUIET_MS;
        assert!(last_output != BASE);
        assert!(ready_to_submit(now, 0, last_output, BASE));
    }

    #[test]
    fn holds_while_the_target_is_still_producing_output() {
        let now = SUBMIT_FLOOR_MS + 500;
        assert!(!ready_to_submit(now, 0, now - 10, BASE));
    }

    #[test]
    fn submits_at_the_ceiling_even_if_the_target_never_speaks() {
        // Backstop for a CLI that echoes nothing at all: without this the
        // has-it-reacted rule would hold the Enter forever.
        assert!(ready_to_submit(SUBMIT_CEILING_MS, 0, BASE, BASE));
    }

    #[test]
    fn quiet_is_measured_from_the_last_output_not_from_the_write() {
        let now = SUBMIT_FLOOR_MS + 1_000;
        assert!(ready_to_submit(now, 0, now - SUBMIT_QUIET_MS, BASE));
        assert!(!ready_to_submit(now, 0, now - (SUBMIT_QUIET_MS - 1), BASE));
    }

    #[test]
    fn only_codex_is_framed_as_a_bracketed_paste() {
        assert!(is_codex("codex"));
        assert!(is_codex("Codex.cmd"));
        assert!(is_codex("CODEX.EXE"));
        // Claude Code already submits reliably and must keep its plain path.
        assert!(!is_codex("claude"));
        assert!(!is_codex("opencode"));
        assert!(!is_codex("powershell.exe"));
    }

    #[test]
    fn is_agent_cli_recognizes_the_wired_clis_and_excludes_shells() {
        assert!(is_agent_cli("claude"));
        assert!(is_agent_cli("Codex.cmd"));
        assert!(is_agent_cli("OPENCODE.EXE"));
        assert!(!is_agent_cli("powershell.exe"));
        assert!(!is_agent_cli("cmd"));
        assert!(!is_agent_cli("bash"));
    }

    #[test]
    fn validate_human_dispatch_trims_and_requires_both_fields() {
        assert_eq!(
            validate_human_dispatch("  sess-2  ", "  do the thing  ").unwrap(),
            ("sess-2".to_string(), "do the thing".to_string())
        );
        assert!(validate_human_dispatch("", "task").is_err());
        assert!(validate_human_dispatch("sess-2", "   ").is_err());
        assert!(validate_human_dispatch("   ", "").is_err());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The session engine is shared with the MCP server so the conductor's
            // dispatch can write straight into a target session's terminal.
            let sessions = Arc::new(SessionManager::default());
            app.manage(sessions.clone());

            // Start the in-process MCP "shared brain" on a random loopback port.
            // Sessions each get their own endpoint; this one is the fallback and
            // the address shown in the sidebar.
            //
            // No agent CLI is registered here. Wiring happens per session, at
            // launch, through arguments and environment only — see
            // agent_mcp_wiring.
            let handle = app.handle().clone();
            // Placeholder until the frontend reports its project. Deriving this
            // from the working directory made the brain's files land wherever
            // the app happened to be started from, which for a packaged build is
            // wherever Explorer felt like.
            let dir = default_context_dir();
            let (port, shared) = mcp::start(handle, dir, sessions)?;
            let url = format!("http://127.0.0.1:{port}/mcp");
            app.manage(McpInfo { url, port });
            app.manage(shared);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            spawn_session,
            write_session,
            resize_session,
            kill_session,
            mcp_info,
            get_context,
            set_agent_brain,
            conductor_state,
            set_conductor,
            halt_conductor,
            set_project,
            human_dispatch
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.state::<Arc<SessionManager>>().kill_all();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
