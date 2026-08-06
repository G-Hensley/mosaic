// Mosaic — multi-session PTY engine.
//
// Each live agent runs on its own pseudo-terminal (ConPTY). Sessions are keyed
// by a client-supplied id so the frontend can address a pane (write/resize/kill)
// the moment it asks to spawn it, while the streaming command keeps running in
// the background. In later milestones this grows a TOML registry + git-worktree
// isolation; here it's the flat, working core.

mod mcp;
mod worktree;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
}

#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionHandle>>,
    /// Serialize ConPTY openpty+spawn: concurrent spawns can stall a PTY pipe on
    /// Windows, so only one session is created at a time.
    spawn_lock: Mutex<()>,
}

impl SessionManager {
    fn kill(&self, id: &str) {
        if let Some(mut h) = self.sessions.lock().unwrap().remove(id) {
            let _ = h.child.kill();
            if let Some(wt) = &h.worktree {
                worktree::remove(wt);
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

    /// Submit a prompt to a terminal UI as typing followed by a distinct Enter
    /// keypress. Codex and Claude Code deliberately distinguish pasted text
    /// containing a carriage return from an interactive Enter event; writing
    /// both in one PTY operation can leave the prompt sitting in the editor.
    pub fn submit_to(&self, id: &str, prompt: &str) -> bool {
        if !self.write_to(id, prompt) {
            return false;
        }

        // Give the terminal UI time to consume the paste before Enter arrives,
        // otherwise ConPTY may coalesce both writes into the same input read.
        thread::sleep(Duration::from_millis(100));
        self.write_to(id, "\r")
    }

    /// Ids of every live session.
    pub fn ids(&self) -> Vec<String> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }

    /// Kill every session — used on window close so no agent process is orphaned
    /// and no worktree is left behind.
    fn kill_all(&self) {
        let mut map = self.sessions.lock().unwrap();
        for (id, mut h) in map.drain() {
            let _ = h.child.kill();
            if let Some(wt) = &h.worktree {
                worktree::remove(wt);
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
            let body = format!(
                r#"{{"mcpServers":{{"mosaic":{{"type":"http","url":"{url}"}}}}}}"#
            );
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
            vec![
                "-c".to_string(),
                format!("mcp_servers.mosaic.url={url}"),
            ],
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
    let (extra_args, extra_env) = match mcp::start_session_server(shared.inner().clone(), session_id.clone())
    {
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
            },
        );
        reader
    };

    // Blocking reads on their own thread → async forward loop via mpsc.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    while let Some(bytes) = rx.recv().await {
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
            worktree::remove(w);
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
            set_project
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.state::<Arc<SessionManager>>().kill_all();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
