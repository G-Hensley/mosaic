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
use std::process::Command;
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
    /// This session's dedicated MCP listener. Present unless the endpoint failed
    /// and the session fell back to the shared one. Aborted on teardown; without
    /// this, one listener leaks per session for the life of the app.
    server: Option<mcp::SessionServer>,
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
///
/// This is the floor for a *small* payload; see `submit_floor_ms`, which raises
/// it in proportion to how much there is to deliver.
const SUBMIT_FLOOR_MS: u64 = 300;

/// Give up waiting for quiet and send Enter regardless. An agent that is already
/// mid-task streams output continuously and would never look quiet, so without a
/// ceiling its dispatch would wait forever. This bounds the wait instead.
///
/// Like the floor, this is the allowance for a small payload; `submit_ceiling_ms`
/// extends it by the same delivery estimate so a big paste is not cut off by a
/// bound that was only ever sized for a small one.
const SUBMIT_CEILING_MS: u64 = 4000;

/// Assumed worst-case rate at which a payload reaches the target, in bytes per
/// millisecond, used to size both bounds against the actual payload.
///
/// This is deliberately far below raw pipe throughput, because the quantity that
/// matters is not how fast ConPTY moves bytes but how fast the target *consumes*
/// them — a TUI that re-renders its composer on every chunk ingests far slower
/// than the pipe delivers. The value is anchored to the live repro: a 1946-byte
/// dispatch was still arriving after 300 ms, so the effective rate is below
/// 1946/300 ≈ 6.5 B/ms. Rounding down to 4 B/ms gives that payload a 486 ms
/// delivery allowance — comfortably past the point it was observed to fail —
/// and keeps the estimate conservative for slower machines.
const SUBMIT_BYTES_PER_MS: u64 = 4;

/// Ceiling on the payload-derived delivery allowance. Without it a pathologically
/// large prompt would scale its own timeout without limit; with it the total wait
/// can never exceed `SUBMIT_CEILING_MS + SUBMIT_DELIVERY_CAP_MS`.
const SUBMIT_DELIVERY_CAP_MS: u64 = 10_000;

/// How often to re-check for quiet while waiting.
const SUBMIT_POLL_MS: u64 = 20;

/// How long `payload_len` bytes could plausibly still be in flight to the target.
///
/// Both bounds are built from this, which is what ties the timing policy to the
/// thing that actually varies. The previous constants were fixed, so a 50-byte
/// prompt and a 50 KB one were given identical windows.
fn delivery_allowance_ms(payload_len: usize) -> u64 {
    ((payload_len as u64) / SUBMIT_BYTES_PER_MS).min(SUBMIT_DELIVERY_CAP_MS)
}

/// Earliest an Enter may be sent for a payload of `payload_len` bytes.
///
/// Raising the floor with payload size is what closes the noisy-pane hole. The
/// "has the target reacted" rule infers reaction from the output clock moving,
/// but a pane that was *already* streaming when the dispatch arrived satisfies
/// that instantly with output that has nothing to do with our paste; a lull in
/// that unrelated stream then reads as "done receiving". Holding until the
/// payload could plausibly have been delivered means such a lull can no longer
/// release an Enter into a paste that is still arriving.
fn submit_floor_ms(payload_len: usize) -> u64 {
    SUBMIT_FLOOR_MS.max(delivery_allowance_ms(payload_len))
}

/// Latest an Enter may be withheld for a payload of `payload_len` bytes.
///
/// Extending the ceiling by the same delivery estimate keeps the backstop from
/// expiring *during* delivery of a large paste, which would fire Enter into the
/// middle of it — the exact failure the ceiling is supposed to be a safe fallback
/// from. Adding the allowance to a base that already exceeds `SUBMIT_FLOOR_MS`
/// also keeps this strictly above `submit_floor_ms` for every input, so the
/// backstop can never preempt the floor.
fn submit_ceiling_ms(payload_len: usize) -> u64 {
    SUBMIT_CEILING_MS + delivery_allowance_ms(payload_len)
}

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
///
/// `payload_len` is the size of what was written, in bytes, and scales both
/// bounds — see `submit_floor_ms` and `submit_ceiling_ms`. Reacting to output
/// alone is not sufficient when the target was already talking before we wrote,
/// so the floor carries the part of the decision that output cannot.
fn ready_to_submit(
    now: u64,
    started: u64,
    last_output: u64,
    baseline: u64,
    payload_len: usize,
) -> bool {
    let waited = now.saturating_sub(started);
    // Checked first so a silent target is always released eventually.
    if waited >= submit_ceiling_ms(payload_len) {
        return true;
    }
    if waited < submit_floor_ms(payload_len) {
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

/// Release everything a live session owns. Shared by explicit kill and by the
/// natural-exit path so the two cannot drift apart — they previously cleaned up
/// slightly different sets of resources.
fn release_session(session_id: &str, handle: SessionHandle) {
    if let Some(server) = handle.server {
        server.shutdown();
    }
    if let Some(worktree) = &handle.worktree {
        report_worktree_cleanup(worktree);
    }
    let _ = std::fs::remove_dir_all(session_config_dir(session_id));
}

/// Owns what a spawn creates before a `SessionHandle` exists to own it: the
/// worktree, the per-session config dir, and the dedicated MCP listener.
/// `spawn_session` can still return early after those are in place (opening the
/// PTY or spawning the child can fail), and dropping this guard tears them back
/// down. Without it a failed spawn strands a worktree, a branch, a config
/// directory, and a live loopback listener with no session attached to any of it.
///
/// Disarmed by `into_session` at the moment the `SessionHandle` takes over.
struct SpawnRollback {
    session_id: String,
    worktree: Option<worktree::Worktree>,
    server: Option<mcp::SessionServer>,
    armed: bool,
}

impl SpawnRollback {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            worktree: None,
            server: None,
            armed: true,
        }
    }

    /// Hand the resources to the session that now owns them, cancelling cleanup.
    fn into_session(mut self) -> (Option<worktree::Worktree>, Option<mcp::SessionServer>) {
        self.armed = false;
        (self.worktree.take(), self.server.take())
    }
}

impl Drop for SpawnRollback {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        eprintln!(
            "[mosaic] spawn failed for {}, rolling back",
            self.session_id
        );
        if let Some(server) = self.server.take() {
            server.shutdown();
        }
        if let Some(worktree) = &self.worktree {
            report_worktree_cleanup(worktree);
        }
        let _ = std::fs::remove_dir_all(session_config_dir(&self.session_id));
    }
}

impl SessionManager {
    fn kill(&self, id: &str) {
        if let Some(mut h) = self.sessions.lock().unwrap().remove(id) {
            let _ = h.child.kill();
            release_session(id, h);
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
        // Measured on the framed payload, not the bare prompt: the markers are
        // bytes the target has to ingest too, and it is the full write that has
        // to have landed before an Enter can mean "submit".
        let payload_len = payload.len();
        if !self.write_to(id, &payload) {
            return false;
        }

        let engine = self.clone();
        let id = id.to_string();
        thread::spawn(move || {
            let started = mono_ms();
            while !ready_to_submit(
                mono_ms(),
                started,
                clock.load(Ordering::Relaxed),
                baseline,
                payload_len,
            ) {
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
/// Env var carrying the bearer token to Codex. Codex takes the NAME of the
/// variable in its config, not the value, which is what we want: the secret
/// travels in the environment while only its label appears in `-c` on the
/// command line. Command lines are readable by any local process; environments
/// are not, and putting the token in an argument would hand it to exactly the
/// observer the token exists to stop.
const CODEX_TOKEN_ENV: &str = "MOSAIC_MCP_TOKEN";

fn agent_mcp_wiring(
    program: &str,
    session_id: &str,
    url: &str,
    token: Option<&str>,
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
            let headers = match token {
                Some(t) => format!(r#","headers":{{"Authorization":"Bearer {t}"}}"#),
                None => String::new(),
            };
            let body = format!(
                r#"{{"mcpServers":{{"mosaic":{{"type":"http","url":"{url}"{headers}}}}}}}"#
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
        "codex" => {
            let mut args = vec!["-c".to_string(), format!("mcp_servers.mosaic.url={url}")];
            let mut env = vec![];
            if let Some(t) = token {
                args.push("-c".to_string());
                args.push(format!(
                    "mcp_servers.mosaic.bearer_token_env_var={CODEX_TOKEN_ENV}"
                ));
                env.push((CODEX_TOKEN_ENV.to_string(), t.to_string()));
            }
            (args, env)
        }
        // OPENCODE_CONFIG is merged over the global config, not swapped for it.
        "opencode" => {
            let path = dir.join("opencode.json");
            let headers = match token {
                Some(t) => format!(r#","headers":{{"Authorization":"Bearer {t}"}}"#),
                None => String::new(),
            };
            let body = format!(
                r#"{{"$schema":"https://opencode.ai/config.json","mcp":{{"mosaic":{{"type":"remote","url":"{url}"{headers}}}}}}}"#
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

/// Why isolation could not be provided. Separate from the message so the
/// frontend can branch on the cause rather than parse prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IsolationReason {
    /// The directory the session would run in is not inside a git repository, so
    /// there is nothing to cut a worktree from. Previously this path did not even
    /// log — the `if let Some(root)` simply fell through and the session ran
    /// shared.
    NotARepository,
    /// The repository was found but `git worktree add` failed.
    WorktreeCreateFailed,
}

/// The detail behind a refused isolated spawn. Carries what a user needs to
/// decide between retrying and deliberately continuing without isolation, which
/// is the choice this error exists to put in front of them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IsolationFailure {
    pub reason: IsolationReason,
    /// The session that was refused, so the frontend can match this to the pane
    /// that asked.
    pub session_id: String,
    /// The directory isolation was requested for.
    pub project: String,
    /// The underlying git/IO error, verbatim. Empty for `NotARepository`, which
    /// has no underlying failure — the precondition simply is not met.
    pub detail: String,
    /// Whether retrying could plausibly succeed. A worktree creation failure is
    /// often transient (a stale lock, a full disk); a non-repository will not
    /// become one by retrying, so offering "retry" there would be a dead end.
    pub retryable: bool,
}

/// Discriminates the one error the frontend must treat specially from every
/// other way a spawn can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpawnErrorKind {
    /// An ordinary spawn failure.
    Failed,
    /// Isolation was requested and could not be provided, so nothing was
    /// spawned. Distinct because the remedy is a user decision, not a retry.
    IsolationUnavailable,
}

/// A spawn failure as the frontend receives it.
///
/// `message` is always populated, so any generic error path still has something
/// to show; `kind` and `isolation` are what let the launcher offer retry or
/// continue-unisolated instead of a dead end.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnError {
    pub kind: SpawnErrorKind,
    pub message: String,
    /// Present only when `kind` is `IsolationUnavailable`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<IsolationFailure>,
}

impl SpawnError {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            kind: SpawnErrorKind::Failed,
            message: message.into(),
            isolation: None,
        }
    }

    fn isolation(
        reason: IsolationReason,
        session_id: &str,
        project: &Path,
        detail: impl Into<String>,
    ) -> Self {
        let detail = detail.into();
        let project = project.to_string_lossy().to_string();
        let message = match reason {
            IsolationReason::NotARepository => format!(
                "Isolation was requested for session {session_id}, but {project} is not inside a \
                 git repository, so no worktree could be created. The session was not started."
            ),
            IsolationReason::WorktreeCreateFailed => format!(
                "Isolation was requested for session {session_id}, but creating a git worktree in \
                 {project} failed: {detail}. The session was not started."
            ),
        };
        Self {
            kind: SpawnErrorKind::IsolationUnavailable,
            message,
            isolation: Some(IsolationFailure {
                reason,
                session_id: session_id.to_string(),
                project,
                detail,
                retryable: matches!(reason, IsolationReason::WorktreeCreateFailed),
            }),
        }
    }
}

/// Lets the existing `.map_err(|e| e.to_string())?` sites keep working, so the
/// structured type is additive rather than a rewrite of every failure path.
impl From<String> for SpawnError {
    fn from(message: String) -> Self {
        Self::failed(message)
    }
}

/// Decide where a session runs, refusing to start at all if isolation was asked
/// for and cannot be delivered.
///
/// This used to fall back to the shared project directory and note it with an
/// `eprintln!` that never reaches the app, so a user who asked for isolation got
/// a session running directly in their project while the UI showed it as
/// isolated. Isolation is a safety property the user explicitly requested; the
/// one thing it must not do is quietly become nothing. Downgrading it is now the
/// user's decision to make, which means it has to reach them as an error.
///
/// Split from `spawn_session` so the policy is testable without a Tauri command:
/// `create` is injected so a test can force the failure branch and keep real
/// worktrees out of the user's app-data directory.
fn resolve_isolation<F>(
    isolate: bool,
    session_id: &str,
    project: &Path,
    create: F,
) -> Result<(PathBuf, Option<worktree::Worktree>), SpawnError>
where
    F: FnOnce(&Path, &str) -> Result<worktree::Worktree, String>,
{
    if !isolate {
        return Ok((project.to_path_buf(), None));
    }
    let Some(root) = worktree::repo_root(project) else {
        return Err(SpawnError::isolation(
            IsolationReason::NotARepository,
            session_id,
            project,
            "",
        ));
    };
    match create(&root, session_id) {
        Ok(w) => Ok((w.path.clone(), Some(w))),
        Err(e) => Err(SpawnError::isolation(
            IsolationReason::WorktreeCreateFailed,
            session_id,
            project,
            e,
        )),
    }
}

/// Which worktree an isolated session should run in: the one it was already
/// working in, or a new one.
///
/// Restoring a pane must not quietly duplicate its worktree. The one from the
/// last run can hold uncommitted agent work — `worktree::remove` refuses to
/// delete a dirty worktree for exactly that reason — and a worktree no pane
/// points at is work the app can no longer lead the user back to. So a fresh one
/// is cut only when the old one is genuinely gone (or belonged to a different
/// project); a directory that is still on disk but unusable refuses the spawn
/// instead, naming the path so the user can recover it.
///
/// `create` is injected so a test can assert it was *not* called.
fn choose_worktree<F>(
    root: &Path,
    session_id: &str,
    saved: Option<&worktree::Saved>,
    create: F,
) -> Result<worktree::Worktree, String>
where
    F: FnOnce(&Path, &str) -> Result<worktree::Worktree, String>,
{
    if let Some(saved) = saved {
        match worktree::reattach(root, saved) {
            worktree::Reattach::Reused(w) => return Ok(w),
            worktree::Reattach::Unusable(why) => {
                return Err(format!(
                    "the worktree {session_id} was working in is still on disk at {} but {why}. \
                     Mosaic will not create a second one, because the first may hold uncommitted \
                     work — recover or delete it, then start the session again",
                    saved.path
                ))
            }
            // Nothing left to strand: cut a fresh one below.
            worktree::Reattach::Missing | worktree::Reattach::Foreign => {}
        }
    }
    create(root, session_id)
}

/// Which worktree a session ended up in, pushed to the frontend so it can
/// remember it and put the session back there after a restart.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionWorktree {
    session_id: String,
    repo: String,
    path: String,
    branch: String,
    base: String,
}

/// Spawn a session under `session_id` and stream its output over `channel` until
/// the child exits. Returns as soon as streaming ends; the frontend fires it
/// without awaiting and addresses the session by the id it supplied.
///
/// Fails closed on isolation: if `isolate` was requested and no worktree could be
/// created, this returns `SpawnErrorKind::IsolationUnavailable` and starts
/// nothing, rather than silently running the session in the shared project dir.
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
    // The worktree this session used before the app was last closed, when the
    // frontend is restoring a remembered pane. See `choose_worktree`.
    reuse_worktree: Option<worktree::Saved>,
) -> Result<(), SpawnError> {
    // Refuse a session id that is already live. Inserting over one would replace
    // the handle without killing the process or removing its worktree, orphaning
    // both. This early check only saves work — it cannot be authoritative,
    // because nothing stops a second spawn passing it before the first inserts.
    // The binding check is inside the spawn lock below.
    if state.sessions.lock().unwrap().contains_key(&session_id) {
        return Err(SpawnError::failed(format!(
            "session {session_id} is already running"
        )));
    }

    // Decide where this session runs: the project dir, or its own git worktree
    // when isolated. Done before taking the spawn lock — creating a worktree and
    // registering MCP takes seconds and shouldn't serialize other spawns.
    let project = cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    // Everything created from here until the SessionHandle exists is owned by this
    // guard, so any early return below unwinds it instead of stranding it. That
    // includes the isolation refusal below, which is why it is armed first.
    let mut rollback = SpawnRollback::new(session_id.clone());
    let (session_cwd, worktree) = resolve_isolation(
        isolate.unwrap_or(false),
        &session_id,
        &project,
        |root, id| choose_worktree(root, id, reuse_worktree.as_ref(), worktree::create),
    )?;
    // Captured before the worktree is handed to the rollback guard, and emitted
    // only once the session is actually live — see below.
    let session_worktree = worktree.as_ref().map(|w| SessionWorktree {
        session_id: session_id.clone(),
        repo: w.repo.to_string_lossy().to_string(),
        path: w.path.to_string_lossy().to_string(),
        branch: w.branch.clone(),
        base: w.base.clone(),
    });
    rollback.worktree = worktree;

    // EVERY session gets its own endpoint, isolated or not. Because that port is
    // only ever handed to this one session, any request arriving on it is
    // provably from it — so identity needs no handshake and can't be spoofed or
    // forgotten. Sessions sharing one endpoint would all authenticate as
    // "unknown", which silently breaks brain assignment and the conductor.
    let (extra_args, extra_env) =
        match mcp::start_session_server(shared.inner().clone(), session_id.clone()) {
            Ok(server) => {
                let url = format!("http://127.0.0.1:{}/mcp", server.port);
                let token = server.token.clone();
                rollback.server = Some(server);
                shared.note_session(&session_id, &program);
                agent_mcp_wiring(&program, &session_id, &url, Some(&token))
            }
            // Endpoint failed: fall back to the shared one. The agent can still reach
            // the brain, it just has to declare who it is.
            //
            // No token here: the shared endpoint is not per-session, so there is
            // nothing session-specific to prove. That endpoint is exactly the
            // self-declared-identity path audit finding #6 also covers, and it is
            // still open.
            Err(e) => {
                eprintln!("[mosaic] session endpoint failed, using shared: {e}");
                agent_mcp_wiring(&program, &session_id, &mcp.url, None)
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

        // Take the reader and writer BEFORE spawning. Both can fail, and spawning
        // is the one step here that is not undoable — doing it last means a
        // failure never leaves a live child process behind for the guard to
        // reap, so the rollback only ever has inert resources to clean up.
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        // The authoritative duplicate check. `spawn_lock` serializes this whole
        // block, so no other spawn can insert between here and the insertion
        // below — which makes check-then-insert atomic against them. It sits
        // before the spawn deliberately: losing this race must not leave a live
        // child behind, and the rollback guard only handles inert resources.
        if state.sessions.lock().unwrap().contains_key(&session_id) {
            return Err(SpawnError::failed(format!(
                "session {session_id} is already running"
            )));
        }

        let mut cmd = build_command(&program, &args, &session_cwd, &extra_env);
        // The agent's Mosaic name = its session id, so the collab skill can
        // self-identify and the app can map it to a brain.
        cmd.env("MOSAIC_SESSION", &session_id);
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave); // so the reader hits EOF when the child exits

        // The session owns the worktree and listener from here on, so the guard
        // stands down. Nothing below this point can fail before insertion.
        let (worktree, server) = rollback.into_session();
        state.sessions.lock().unwrap().insert(
            session_id.clone(),
            SessionHandle {
                master: pair.master,
                writer,
                child,
                worktree,
                last_output: activity.clone(),
                program: program.clone(),
                server,
            },
        );
        reader
    };

    // Now that the session is live, tell the frontend which worktree it got, so a
    // restart can put this pane back into it. Deliberately after insertion: a
    // spawn that failed earlier has had its worktree rolled back, and the
    // frontend must not be left remembering one that no longer exists.
    if let Some(event) = session_worktree {
        let _ = app.emit("session-worktree", event);
    }

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
    // Worktree without removing it and leave the session's MCP listener serving,
    // stranding a directory, a branch, and a port for every session that wasn't
    // closed by hand.
    let handle = state.sessions.lock().unwrap().remove(&session_id);
    if let Some(h) = handle {
        release_session(&session_id, h);
    } else {
        // Already removed by an explicit kill, which cleaned the rest.
        let _ = std::fs::remove_dir_all(session_config_dir(&session_id));
    }
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

#[tauri::command]
fn project_is_repo(dir: String) -> bool {
    worktree::repo_root(Path::new(&dir)).is_some()
}

#[tauri::command]
fn init_project_repo(dir: String) -> Result<(), String> {
    let output = Command::new("git")
        .arg("init")
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("failed to run git init in {dir}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("git init failed in {dir}: {detail}"))
    }
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

    // No reviewer: this is a human typing into the UI, and a human dispatching
    // work directly is already the second pair of eyes the gate exists to
    // provide. Give this a reviewer field when the UI grows somewhere to pick
    // one, rather than guessing a session on the user's behalf.
    shared.dispatch_task(&from, &target, &task, "")
}

#[cfg(test)]
mod tests {
    use super::{
        agent_mcp_wiring, choose_worktree, delivery_allowance_ms, is_agent_cli, is_codex,
        ready_to_submit, resolve_isolation, submit_ceiling_ms, submit_floor_ms,
        validate_human_dispatch, worktree, IsolationReason, SpawnErrorKind, SpawnRollback,
        CODEX_TOKEN_ENV, SUBMIT_BYTES_PER_MS, SUBMIT_CEILING_MS, SUBMIT_DELIVERY_CAP_MS,
        SUBMIT_FLOOR_MS, SUBMIT_QUIET_MS,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// A temp repo plus one worktree created inside it, standing in for the state
    /// a spawn has already built when a later step fails.
    fn worktree_fixture(tmp: &TempDir, session_id: &str) -> worktree::Worktree {
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        worktree::init_test_repo(&repo).unwrap();
        worktree::create_with_base_dir(&repo, session_id, tmp.path()).unwrap()
    }

    #[test]
    fn an_armed_rollback_removes_the_worktree_a_failed_spawn_left_behind() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-rollback-armed");
        let path = wt.path.clone();
        assert!(path.exists(), "fixture should start on disk");

        {
            let mut rollback = SpawnRollback::new("sess-rollback-armed".into());
            rollback.worktree = Some(wt);
            // Dropped here, standing in for an early return from spawn_session.
        }

        assert!(!path.exists(), "rollback should remove the worktree");
    }

    #[test]
    fn a_rollback_disarmed_by_into_session_leaves_the_worktree_alone() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-rollback-disarmed");
        let path = wt.path.clone();

        let handed_over = {
            let mut rollback = SpawnRollback::new("sess-rollback-disarmed".into());
            rollback.worktree = Some(wt);
            let (worktree, _server) = rollback.into_session();
            worktree
        };

        assert!(
            path.exists(),
            "a session that spawned successfully must keep its worktree"
        );
        assert!(
            handed_over.is_some(),
            "the worktree should be handed to the SessionHandle, not dropped"
        );
    }

    #[test]
    fn rollback_preserves_a_worktree_that_already_has_uncommitted_work() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-rollback-dirty");
        let path = wt.path.clone();
        fs::write(path.join("in-progress.txt"), "unsaved").unwrap();

        {
            let mut rollback = SpawnRollback::new("sess-rollback-dirty".into());
            rollback.worktree = Some(wt);
        }

        assert!(
            path.join("in-progress.txt").exists(),
            "rollback must not discard uncommitted work"
        );
    }

    // ---- Restoring an isolated pane ----
    //
    // The failure these guard against is silent: a restored session cuts itself a
    // second worktree, and the one holding the agent's uncommitted work is left
    // with nothing pointing at it.

    fn saved_from(wt: &worktree::Worktree) -> worktree::Saved {
        worktree::Saved {
            repo: wt.repo.to_string_lossy().to_string(),
            path: wt.path.to_string_lossy().to_string(),
            branch: wt.branch.clone(),
            base: wt.base.clone(),
        }
    }

    /// Stands in for `worktree::create` where creating one would be the bug.
    fn create_must_not_be_called(
        _repo: &Path,
        _session_id: &str,
    ) -> Result<worktree::Worktree, String> {
        panic!("a second worktree must not be created while the first is on disk");
    }

    #[test]
    fn a_restored_session_goes_back_into_its_own_worktree() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-restore");
        let repo = wt.repo.clone();
        fs::write(wt.path.join("in-progress.txt"), "unsaved").unwrap();

        let chosen = choose_worktree(
            &repo,
            "sess-restore",
            Some(&saved_from(&wt)),
            create_must_not_be_called,
        )
        .expect("an intact worktree should be reused");

        assert_eq!(chosen.path, wt.path);
        assert!(
            chosen.path.join("in-progress.txt").exists(),
            "the restored session must land on the work it left behind"
        );
    }

    #[test]
    fn a_restored_session_gets_a_new_worktree_only_when_the_old_one_is_gone() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-clean-exit");
        let repo = wt.repo.clone();
        let saved = saved_from(&wt);
        // Last run ended clean, so cleanup took the worktree with it.
        worktree::remove(&wt).unwrap();

        let base = tmp.path().join("fresh");
        let chosen = choose_worktree(&repo, "sess-clean-exit", Some(&saved), |r, s| {
            worktree::create_with_base_dir(r, s, &base)
        })
        .expect("a vanished worktree should be replaced");

        assert!(chosen.path.exists());
        assert_ne!(chosen.path, wt.path);
    }

    #[test]
    fn an_unusable_worktree_refuses_the_spawn_rather_than_stranding_it() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-unusable");
        let repo = wt.repo.clone();

        // On disk, holding work, but not registered with this repo any more.
        let orphan = tmp.path().join("orphan");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("in-progress.txt"), "unsaved").unwrap();
        let saved = worktree::Saved {
            repo: repo.to_string_lossy().to_string(),
            path: orphan.to_string_lossy().to_string(),
            branch: "mosaic/sess-unusable".to_string(),
            base: wt.base.clone(),
        };

        // `Worktree` has no Debug, so `expect_err` is unavailable here.
        let err = match choose_worktree(
            &repo,
            "sess-unusable",
            Some(&saved),
            create_must_not_be_called,
        ) {
            Err(e) => e,
            Ok(_) => panic!("a worktree still on disk must not be silently abandoned"),
        };

        assert!(
            err.contains(&orphan.to_string_lossy().to_string()),
            "the message must name the path so the work can be recovered: {err}"
        );
        assert!(orphan.join("in-progress.txt").exists());
    }

    #[test]
    fn switching_projects_leaves_the_old_project_worktree_alone() {
        let tmp = TempDir::new().unwrap();
        let wt = worktree_fixture(&tmp, "sess-moved");

        // A different repo is now the project; the saved worktree belongs to the
        // old one, which still tracks it.
        let other = tmp.path().join("other");
        fs::create_dir_all(&other).unwrap();
        worktree::init_test_repo(&other).unwrap();

        let base = tmp.path().join("fresh");
        let chosen = choose_worktree(&other, "sess-moved", Some(&saved_from(&wt)), |r, s| {
            worktree::create_with_base_dir(r, s, &base)
        })
        .expect("a session in a new project gets a worktree of that project");

        assert_ne!(chosen.path, wt.path);
        assert!(
            wt.path.exists(),
            "the other project's worktree is untouched"
        );
    }

    // Isolation must fail closed. It used to fall back to the shared project
    // directory and note it with an eprintln! that never reaches the app, so a
    // user who asked for isolation got a session running loose in their project
    // while the UI showed it as isolated — a safety property they explicitly
    // asked for, silently downgraded to nothing.

    /// Stands in for `worktree::create` when a test needs the failure branch.
    /// Injected rather than provoked so the case is deterministic and no real
    /// worktree is created in the user's app-data directory.
    fn create_fails(_repo: &Path, _session_id: &str) -> Result<worktree::Worktree, String> {
        Err("fatal: could not lock ref: File exists".into())
    }

    /// `worktree::Worktree` deliberately has no `Debug`, so `expect_err` cannot be
    /// used on what `resolve_isolation` returns.
    fn expect_refusal(
        result: Result<(PathBuf, Option<worktree::Worktree>), super::SpawnError>,
        context: &str,
    ) -> super::SpawnError {
        match result {
            Ok(_) => panic!("{context}"),
            Err(e) => e,
        }
    }

    #[test]
    fn isolation_requested_but_unavailable_refuses_to_spawn() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        worktree::init_test_repo(&repo).unwrap();

        let err = expect_refusal(
            resolve_isolation(true, "sess-iso-fail", &repo, create_fails),
            "a session that asked for isolation must not start without it",
        );

        assert_eq!(err.kind, SpawnErrorKind::IsolationUnavailable);
        let detail = err.isolation.expect("the frontend needs the cause");
        assert_eq!(detail.reason, IsolationReason::WorktreeCreateFailed);
        assert_eq!(detail.session_id, "sess-iso-fail");
        assert!(
            detail.retryable,
            "a lock failure is transient, so retry must be offered"
        );
        // The underlying git error has to survive to the user, otherwise the
        // dialog can only say that something went wrong.
        assert!(
            detail.detail.contains("could not lock ref"),
            "underlying error must reach the frontend, got {:?}",
            detail.detail
        );
        assert!(
            err.message.contains("not started"),
            "the message must say nothing was spawned, got {:?}",
            err.message
        );
    }

    // The path that did not even log: `repo_root` returning None skipped the
    // whole isolation block and the session ran shared in silence.
    #[test]
    fn isolation_requested_outside_a_repository_refuses_to_spawn() {
        let tmp = TempDir::new().unwrap();
        let plain = tmp.path().join("not-a-repo");
        fs::create_dir_all(&plain).unwrap();

        let err = expect_refusal(
            resolve_isolation(true, "sess-iso-norepo", &plain, |_, _| {
                panic!("must not attempt to create a worktree outside a repository")
            }),
            "no repository means no isolation, so no spawn",
        );

        assert_eq!(err.kind, SpawnErrorKind::IsolationUnavailable);
        let detail = err.isolation.expect("the frontend needs the cause");
        assert_eq!(detail.reason, IsolationReason::NotARepository);
        assert!(
            !detail.retryable,
            "a directory will not become a repository by retrying"
        );
    }

    #[test]
    fn project_repo_check_distinguishes_repo_from_plain_directory() {
        let tmp = TempDir::new().unwrap();
        let plain = tmp.path().join("plain");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&plain).unwrap();
        fs::create_dir_all(&repo).unwrap();
        worktree::init_test_repo(&repo).unwrap();

        assert!(!super::project_is_repo(
            plain.to_string_lossy().into_owned()
        ));
        assert!(super::project_is_repo(repo.to_string_lossy().into_owned()));
    }

    #[test]
    fn isolation_requested_and_available_runs_in_the_worktree() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        worktree::init_test_repo(&repo).unwrap();
        let base = tmp.path().to_path_buf();

        let (cwd, worktree) = resolve_isolation(true, "sess-iso-ok", &repo, |r, s| {
            worktree::create_with_base_dir(r, s, &base)
        })
        .expect("isolation that can be provided must still spawn");

        let wt = worktree.expect("the session must own its worktree for cleanup");
        assert_eq!(cwd, wt.path, "the session must run inside its worktree");
        assert_ne!(cwd, repo, "an isolated session must not run in the project");
        assert!(cwd.exists());
    }

    #[test]
    fn a_session_that_did_not_ask_for_isolation_is_unaffected() {
        let tmp = TempDir::new().unwrap();
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();

        // Not a repository, and no worktree may be attempted — an unisolated
        // session must not acquire a git requirement it never had.
        let (cwd, worktree) = resolve_isolation(false, "sess-plain", &plain, |_, _| {
            panic!("must not create a worktree when isolation was not requested")
        })
        .expect("an unisolated session must still spawn");

        assert_eq!(cwd, plain, "it runs in the project directory, as before");
        assert!(worktree.is_none());
    }

    // Pins the wire contract the launcher UI is being built against.
    #[test]
    fn the_isolation_error_serializes_with_the_fields_the_frontend_branches_on() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        worktree::init_test_repo(&repo).unwrap();

        let err = expect_refusal(
            resolve_isolation(true, "sess-wire", &repo, create_fails),
            "isolation must be refused here",
        );
        let json = serde_json::to_value(&err).unwrap();

        assert_eq!(json["kind"], "isolationUnavailable");
        assert!(json["message"].is_string());
        assert_eq!(json["isolation"]["reason"], "worktreeCreateFailed");
        assert_eq!(json["isolation"]["sessionId"], "sess-wire");
        assert_eq!(json["isolation"]["retryable"], true);
        assert!(json["isolation"]["project"].is_string());
        assert!(json["isolation"]["detail"].is_string());

        // An ordinary failure stays distinguishable, so the launcher only offers
        // the isolation choice when isolation is actually what went wrong.
        let ordinary = serde_json::to_value(super::SpawnError::failed("boom")).unwrap();
        assert_eq!(ordinary["kind"], "failed");
        assert_eq!(ordinary["message"], "boom");
        assert!(
            ordinary.get("isolation").is_none(),
            "a non-isolation failure must not carry an isolation payload"
        );
    }

    // A dispatch is only lost in one direction: an Enter sent too early is
    // swallowed silently, while one sent late costs nothing but a pause. Every
    // case below is therefore written from the "hold unless certain" side.
    //
    // `baseline` is the output clock as of the write. Cases where it still equals
    // `last_output` are the target not having reacted yet.

    // Stands in for the output clock as of the write. Kept below the `now` values
    // used here so the cases stay arithmetically honest.
    const BASE: u64 = 100;

    // A payload small enough to add no delivery allowance, so the cases below
    // exercise the quiet/reacted rules against the unscaled base bounds. Size
    // scaling is covered separately further down.
    const SMALL: usize = 0;

    // The payload from the live repro: a ~1900-character dispatch that sat in
    // Codex's composer as "[Pasted Content 1946 chars]", unsubmitted.
    const REPRO: usize = 1946;

    #[test]
    fn holds_below_the_floor() {
        assert!(!ready_to_submit(
            SUBMIT_FLOOR_MS - 1,
            0,
            BASE + 1,
            BASE,
            SMALL
        ));
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
        assert!(!ready_to_submit(now, 0, BASE, BASE, SMALL));
    }

    #[test]
    fn submits_once_the_target_has_spoken_and_then_gone_quiet() {
        let now = SUBMIT_FLOOR_MS + SUBMIT_QUIET_MS;
        let last_output = now - SUBMIT_QUIET_MS;
        assert!(last_output != BASE);
        assert!(ready_to_submit(now, 0, last_output, BASE, SMALL));
    }

    #[test]
    fn holds_while_the_target_is_still_producing_output() {
        let now = SUBMIT_FLOOR_MS + 500;
        assert!(!ready_to_submit(now, 0, now - 10, BASE, SMALL));
    }

    #[test]
    fn submits_at_the_ceiling_even_if_the_target_never_speaks() {
        // Backstop for a CLI that echoes nothing at all: without this the
        // has-it-reacted rule would hold the Enter forever.
        assert!(ready_to_submit(SUBMIT_CEILING_MS, 0, BASE, BASE, SMALL));
    }

    #[test]
    fn quiet_is_measured_from_the_last_output_not_from_the_write() {
        let now = SUBMIT_FLOOR_MS + 1_000;
        assert!(ready_to_submit(now, 0, now - SUBMIT_QUIET_MS, BASE, SMALL));
        assert!(!ready_to_submit(
            now,
            0,
            now - (SUBMIT_QUIET_MS - 1),
            BASE,
            SMALL
        ));
    }

    // Size scaling. Both bounds were fixed constants, so a 50-byte prompt and a
    // 50 KB one were given identical windows — the payload is the one input that
    // actually varies between dispatches.

    #[test]
    fn a_small_payload_keeps_the_original_bounds() {
        // Nothing that fit comfortably before may start waiting longer now.
        assert_eq!(submit_floor_ms(SMALL), SUBMIT_FLOOR_MS);
        assert_eq!(submit_ceiling_ms(SMALL), SUBMIT_CEILING_MS);
        // Anything under the floor's worth of bytes is still governed by the floor.
        let below = (SUBMIT_FLOOR_MS * SUBMIT_BYTES_PER_MS) as usize;
        assert_eq!(submit_floor_ms(below), SUBMIT_FLOOR_MS);
    }

    #[test]
    fn the_repro_payload_is_held_past_the_delay_it_was_seen_to_lose_enter_at() {
        // The dispatch that failed live was still arriving after 300 ms, so a
        // payload that size must not be released at 300 ms.
        let floor = submit_floor_ms(REPRO);
        assert!(
            floor > SUBMIT_FLOOR_MS,
            "a {REPRO}-byte payload must outwait the small-payload floor, got {floor}"
        );
        assert!(!ready_to_submit(SUBMIT_FLOOR_MS, 0, BASE + 1, BASE, REPRO));
    }

    // The hole that survived the quiet-detection fix. A pane that was already
    // streaming when the dispatch arrived moves its output clock off `baseline`
    // immediately, with output that has nothing to do with our paste — so the
    // has-it-reacted rule is satisfied by noise. A lull in that unrelated stream
    // then looks exactly like "finished receiving", and before the floor scaled
    // with size it would have fired an Enter into a paste still in delivery.
    #[test]
    fn a_lull_in_an_already_noisy_pane_cannot_release_a_large_paste_early() {
        // Sampled between the two floors, where the old fixed bound would have
        // released and the scaled one still holds. Derived rather than hardcoded
        // so retuning the rate cannot quietly empty this window.
        let now = SUBMIT_FLOOR_MS + (submit_floor_ms(REPRO) - SUBMIT_FLOOR_MS) / 2;
        assert!(now >= SUBMIT_FLOOR_MS && now < submit_floor_ms(REPRO));
        let last_output = now - SUBMIT_QUIET_MS;
        // The two rules that are supposed to protect us both read as satisfied:
        // the target has "reacted", and it has been quiet long enough.
        assert!(last_output != BASE);
        assert!(now - last_output >= SUBMIT_QUIET_MS);
        // Same instant, same clocks: a small payload goes, a large one is held.
        assert!(ready_to_submit(now, 0, last_output, BASE, SMALL));
        assert!(!ready_to_submit(now, 0, last_output, BASE, REPRO));
        // And it does go once the payload could actually have been delivered.
        let after = submit_floor_ms(REPRO) + SUBMIT_QUIET_MS;
        assert!(ready_to_submit(
            after,
            0,
            after - SUBMIT_QUIET_MS,
            BASE,
            REPRO
        ));
    }

    #[test]
    fn the_ceiling_never_expires_while_a_large_payload_is_still_arriving() {
        // The backstop is a fallback from the bug, so it must not reproduce it by
        // firing mid-delivery. It has to outlast the floor for every payload.
        for len in [SMALL, 1, REPRO, 64 * 1024, 8 * 1024 * 1024] {
            assert!(
                submit_ceiling_ms(len) > submit_floor_ms(len),
                "ceiling must outlast the floor at {len} bytes"
            );
        }
    }

    #[test]
    fn the_wait_stays_bounded_however_large_the_payload() {
        // Scaling must not become an unbounded timeout: a pathological prompt
        // still has to release Enter, and dispatch must not hang on it.
        let huge = usize::MAX;
        assert_eq!(delivery_allowance_ms(huge), SUBMIT_DELIVERY_CAP_MS);
        assert_eq!(
            submit_ceiling_ms(huge),
            SUBMIT_CEILING_MS + SUBMIT_DELIVERY_CAP_MS
        );
        assert!(ready_to_submit(
            SUBMIT_CEILING_MS + SUBMIT_DELIVERY_CAP_MS,
            0,
            BASE,
            BASE,
            huge
        ));
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

    // The token is useless if it never reaches the agent, and each CLI takes it
    // a different way. These pin the three shapes so a config-format change
    // fails here rather than silently at runtime, where the symptom is a
    // session that connects and then 401s on every tool call.

    #[test]
    fn claude_carries_the_token_as_an_authorization_header() {
        let (args, env) = agent_mcp_wiring(
            "claude",
            "sess-tok-claude",
            "http://127.0.0.1:1/mcp",
            Some("secret123"),
        );
        let path = args
            .iter()
            .position(|a| a == "--mcp-config")
            .map(|i| args[i + 1].clone())
            .expect("claude gets a --mcp-config path");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(
            body.contains(r#""Authorization":"Bearer secret123""#),
            "{body}"
        );
        assert!(
            env.is_empty(),
            "claude reads the token from its config file"
        );
    }

    #[test]
    fn codex_passes_the_token_by_env_var_name_not_by_value() {
        let (args, env) = agent_mcp_wiring(
            "codex",
            "sess-tok-codex",
            "http://127.0.0.1:1/mcp",
            Some("secret123"),
        );
        let joined = args.join(" ");
        assert!(
            joined.contains(&format!("bearer_token_env_var={CODEX_TOKEN_ENV}")),
            "{joined}"
        );
        // The whole point: the secret is in the environment, never on the
        // command line, which any local process can read.
        assert!(
            !joined.contains("secret123"),
            "token leaked into argv: {joined}"
        );
        assert!(env.contains(&(CODEX_TOKEN_ENV.to_string(), "secret123".to_string())));
    }

    #[test]
    fn opencode_carries_the_token_as_an_authorization_header() {
        let (_, env) = agent_mcp_wiring(
            "opencode",
            "sess-tok-oc",
            "http://127.0.0.1:1/mcp",
            Some("secret123"),
        );
        let path = env
            .iter()
            .find(|(k, _)| k == "OPENCODE_CONFIG")
            .map(|(_, v)| v.clone())
            .expect("opencode gets OPENCODE_CONFIG");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(
            body.contains(r#""Authorization":"Bearer secret123""#),
            "{body}"
        );
    }

    #[test]
    fn no_token_means_no_auth_stanza_anywhere() {
        // The shared-endpoint fallback has no per-session secret to present.
        // It must still produce a config the agent can load, not one with an
        // empty or malformed Authorization header.
        let (args, _) = agent_mcp_wiring("claude", "sess-tok-none", "http://127.0.0.1:1/mcp", None);
        let path = args
            .iter()
            .position(|a| a == "--mcp-config")
            .map(|i| args[i + 1].clone())
            .unwrap();
        let body = std::fs::read_to_string(path).unwrap();
        assert!(!body.contains("Authorization"), "{body}");
        serde_json::from_str::<serde_json::Value>(&body).expect("still valid JSON without a token");

        let (args, env) =
            agent_mcp_wiring("codex", "sess-tok-none2", "http://127.0.0.1:1/mcp", None);
        assert!(!args.join(" ").contains("bearer_token_env_var"));
        assert!(env.is_empty());
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
            project_is_repo,
            init_project_repo,
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
