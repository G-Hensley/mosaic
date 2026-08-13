// Mosaic "shared brain": an in-process MCP server on loopback that every agent
// CLI connects to. Agents publish decisions/facts/broadcasts and read the shared
// context, so one agent's decision instantly becomes another's knowledge. The
// tool handlers touch app state directly (same process), and each write emits a
// `context-changed` event so the sidebar updates live.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::response::IntoResponse;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

fn dispatch_prompt(conductor: &str, task_id: &str, task: &str) -> String {
    // Keep the injection on one terminal line. Embedded CR/LF characters can
    // become unintended submit events in a target CLI. Preserve all other
    // whitespace because quoted commands and code fragments may depend on it.
    let task = task.replace("\r\n", " ").replace(['\r', '\n'], " ");
    format!(
        "[mosaic] Task from conductor '{conductor}' (task_id {task_id}): {task} When finished, call the mosaic complete_task tool with task_id \"{task_id}\" and your result."
    )
}

/// What a pane is told, in its own terminal, at the moment it becomes conductor.
///
/// This is *typed into the composer and left there unsent* — see `set_conductor`
/// for why. That shapes the text: it has to be short enough to read at a glance
/// and to type after, because the user is expected to append their actual first
/// instruction to it and send both together.
///
/// So this carries only what MCP cannot: the role is live *now*, and who is
/// actually running. The playbook — how to write a task, recording decisions
/// first, subagents versus sessions — is already in BRAIN_INSTRUCTIONS, which
/// every agent receives on connect. Repeating it here just buried the two facts
/// that were new.
fn conductor_briefing(peers: &[String]) -> String {
    // Single line: a newline lands in most composers as a submit, which would
    // fire this off half-written — exactly what we are avoiding.
    let roster = if peers.is_empty() {
        "No other sessions are open yet (Ctrl+K opens one).".to_string()
    } else {
        format!(
            "Live sessions you can dispatch to: {}.",
            // Just id and model. `roster_lines` also carries brain= and the
            // conductor marker, which are useful in list_sessions output but are
            // noise in a line the user has to read and type around; the agent can
            // call list_sessions for the full picture.
            peers
                .iter()
                .map(|l| {
                    let l = l.trim_start_matches("- ").replace('\n', " ");
                    match l.find(" brain=") {
                        Some(i) => l[..i].to_string(),
                        None => l,
                    }
                })
                .collect::<Vec<_>>()
                .join("; ")
        )
    };
    format!(
        "[mosaic] You are now the conductor of this workspace. \
{roster} \
Each is a live agent idling until you give it work. Use the mosaic dispatch tool rather than doing separable work yourself; it returns immediately, so fan out every independent piece and collect with get_task_result. "
    )
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Entry {
    pub kind: String, // "decision" | "fact" | "broadcast"
    pub author: String,
    pub topic: String,
    pub body: String,
    pub ts_ms: u64,
    pub room: String, // which brain this belongs to
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct AgentSession {
    pub name: String,
    pub kind: String,
}

/// One dispatched unit of work, from the conductor to another session.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub from: String,
    pub target: String,
    pub task: String,
    /// "pending" | "overdue" | "done" | "cancelled" | "error".
    /// "overdue" is non-terminal: still running, result still accepted.
    pub status: String,
    pub result: String,
    pub ts_ms: u64,
}

const STORE_FILE: &str = "brain.jsonl";

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum StoreRecord {
    Entry(Entry),
    Session(AgentSession),
    Task(Task),
}

#[derive(Default)]
struct StoredBrain {
    entries: Vec<Entry>,
    sessions: Vec<AgentSession>,
    tasks: Vec<Task>,
}

fn load_brain(dir: &Path) -> StoredBrain {
    let Ok(contents) = fs::read_to_string(dir.join(STORE_FILE)) else {
        return StoredBrain::default();
    };
    let mut brain = StoredBrain::default();
    for record in contents
        .lines()
        .filter_map(|line| serde_json::from_str::<StoreRecord>(line).ok())
    {
        match record {
            StoreRecord::Entry(entry) => brain.entries.push(entry),
            StoreRecord::Session(session) => {
                if let Some(existing) = brain.sessions.iter_mut().find(|s| s.name == session.name) {
                    *existing = session;
                } else {
                    brain.sessions.push(session);
                }
            }
            StoreRecord::Task(task) => {
                if let Some(existing) = brain.tasks.iter_mut().find(|t| t.id == task.id) {
                    *existing = task;
                } else {
                    brain.tasks.push(task);
                }
            }
        }
    }
    brain
}

fn append_record(dir: &Path, record: &StoreRecord) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut line = serde_json::to_string(record).map_err(std::io::Error::other)?;
    line.push('\n');
    append_line(&dir.join(STORE_FILE), &line)
}

/// Guardrails. Note that depth is bounded structurally: only the conductor may
/// dispatch, so a dispatched agent cannot dispatch onward.
const MAX_DISPATCHES: u32 = 40;

/// How long before a still-running task is reported as "overdue".
///
/// This is a reporting threshold, not a deadline. Nothing here cancels an
/// agent: the dispatched CLI keeps working, and its `complete_task` call is
/// still accepted afterwards. Crossing this line only changes what the
/// conductor is told, so that a slow task is visible without its result being
/// thrown away.
///
/// Twenty minutes because real dispatched work routinely runs past ten. A
/// threshold most tasks trip is noise, and noise gets ignored.
const TASK_OVERDUE_MS: u64 = 20 * 60 * 1000;

/// A task record was created and delivery was attempted. `delivered` is false
/// when the prompt could not be written to the target's terminal — the task
/// still exists, in "error" status, rather than vanishing silently the way a
/// failed dispatch used to.
#[derive(Clone, Serialize)]
pub struct DispatchOutcome {
    pub task_id: String,
    pub delivered: bool,
}

/// The checks a dispatch must pass before any task record is created, in the
/// order they're applied — that order is what decides which message wins when
/// more than one would refuse. Kept pure (no lock, no I/O) so it's testable
/// without a live `Shared`, which needs a real `AppHandle` to construct.
///
/// The dispatch budget is deliberately NOT one of these checks: consuming it
/// has to be atomic with checking it (or two concurrent dispatches could both
/// pass), so `dispatch_task` calls `take_dispatch_budget` itself, last, under
/// its own lock.
fn dispatch_precheck(
    halted: bool,
    from: &str,
    target: &str,
    target_is_live: bool,
) -> Result<(), String> {
    if halted {
        return Err("dispatch is halted by the user (Stop). Do not retry.".to_string());
    }
    if target == from {
        return Err("cannot dispatch to yourself.".to_string());
    }
    if !target_is_live {
        return Err(format!(
            "no live session '{target}'. Call list_sessions for valid targets."
        ));
    }
    Ok(())
}

/// The shared store — one instance, cloned by Arc into every agent's handler.
pub struct Shared {
    app: AppHandle,
    /// Where entries are mirrored as markdown. Follows the picked project, so it
    /// changes at runtime rather than being fixed at startup.
    dir: Mutex<PathBuf>,
    entries: Mutex<Vec<Entry>>,
    sessions: Mutex<Vec<AgentSession>>,
    /// agent name -> brain (room). The app owns this; drag reassigns it live.
    name_to_room: Mutex<HashMap<String, String>>,
    /// The live session engine — lets dispatch type into a target's terminal.
    engine: Arc<crate::SessionManager>,
    /// Which agent (if any) is the conductor. Set by the app, never self-claimed.
    conductor: Mutex<Option<String>>,
    /// Global kill-switch for all dispatch.
    halted: Mutex<bool>,
    tasks: Mutex<Vec<Task>>,
    dispatches: Mutex<u32>,
}

impl Shared {
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Re-point storage (the user picked a different project) and rehydrate it.
    pub fn set_dir(&self, dir: PathBuf) {
        let brain = load_brain(&dir);
        *self.dir.lock().unwrap() = dir;
        *self.entries.lock().unwrap() = brain.entries;
        *self.sessions.lock().unwrap() = brain.sessions;
        *self.tasks.lock().unwrap() = brain.tasks;
        let _ = self.app.emit("context-changed", ());
        let _ = self.app.emit("conductor-changed", ());
    }

    /// The brain a given agent name is currently in. Defaults to "main".
    pub fn room_for(&self, name: &str) -> String {
        self.name_to_room
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| "main".to_string())
    }

    /// Assign an agent name to a brain. Called by the app on spawn and on drag,
    /// so re-homing a running agent takes effect on its next tool call.
    fn try_set_room(&self, name: &str, room: &str) -> Result<(), String> {
        validate_path_component("room", room)?;
        self.name_to_room
            .lock()
            .unwrap()
            .insert(name.to_string(), room.to_string());
        let _ = self.app.emit("context-changed", ());
        Ok(())
    }

    pub fn set_room(&self, name: &str, room: &str) {
        let _ = self.try_set_room(name, room);
    }

    /// Append an entry (tagged with the author's current brain) to memory + its
    /// human-readable markdown file, then notify the UI.
    fn add(&self, kind: &str, author: &str, topic: &str, body: &str) {
        let room = self.room_for(author);
        let entry = Entry {
            kind: kind.to_string(),
            author: author.to_string(),
            topic: topic.to_string(),
            body: body.to_string(),
            ts_ms: Self::now_ms(),
            room: room.clone(),
        };
        let dir = self.dir.lock().unwrap().clone();
        let _ = fs::create_dir_all(&dir);
        let file = dir.join(format!("{room}-{kind}s.md"));
        let _ = append_line(&file, &format!("- **{topic}** ({author}): {body}\n"));
        let _ = append_record(&dir, &StoreRecord::Entry(entry.clone()));
        self.entries.lock().unwrap().push(entry);
        let _ = self.app.emit("context-changed", ());
    }

    pub fn entries_snapshot(&self) -> Vec<Entry> {
        self.entries.lock().unwrap().clone()
    }

    pub fn sessions_snapshot(&self) -> Vec<AgentSession> {
        self.sessions.lock().unwrap().clone()
    }

    /// Record a session the app already knows about (dedicated endpoint), so it
    /// shows up in list_sessions without the agent announcing itself.
    pub fn note_session(&self, name: &str, kind: &str) {
        let mut s = self.sessions.lock().unwrap();
        if !s.iter().any(|a| a.name == name) {
            let session = AgentSession {
                name: name.to_string(),
                kind: kind.to_string(),
            };
            let dir = self.dir.lock().unwrap().clone();
            let _ = append_record(&dir, &StoreRecord::Session(session.clone()));
            s.push(session);
        }
        drop(s);
        let _ = self.app.emit("context-changed", ());
    }

    // ---- conductor ----

    /// One line per live session: id, kind, brain, and role. Shared by
    /// `list_sessions` and the conductor briefing so an agent sees the same
    /// picture of the workspace however it asks.
    ///
    /// Sorted by (length, text) rather than plain lexical order, which keeps
    /// `sess-9` ahead of `sess-10`. Session ids come out of a HashMap, so
    /// without this the roster reshuffles between calls and an agent reading it
    /// twice cannot tell a reordering from a membership change.
    pub fn roster_lines(&self) -> Vec<String> {
        let identified = self.sessions_snapshot();
        let conductor = self.conductor();
        let mut ids = self.engine.ids();
        ids.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        ids.iter()
            .map(|id| {
                let kind = identified
                    .iter()
                    .find(|a| &a.name == id)
                    .map(|a| a.kind.clone())
                    .unwrap_or_else(|| "unidentified".to_string());
                let room = self.room_for(id);
                let role = if conductor.as_deref() == Some(id.as_str()) {
                    " [conductor]"
                } else {
                    ""
                };
                format!("- {id} ({kind}) brain={room}{role}")
            })
            .collect()
    }

    /// Promote (or, with `None`, demote) a pane.
    ///
    /// Promotion also briefs the agent in its own terminal. That injection is
    /// the whole point rather than a nicety: MCP hands a server's instructions
    /// to a client once, at connect time, but which pane is the conductor is
    /// decided by the user long afterwards and can change during a run. An agent
    /// promoted at minute ten has therefore never been told it now commands
    /// every other pane, and the observed result is that it just keeps working
    /// alone. The terminal is the only channel that reaches a *running* agent,
    /// so the role change is delivered the moment it becomes true.
    ///
    /// The briefing is typed into the composer but deliberately NOT submitted.
    /// Auto-sending it made promotion silently spend a turn on a prompt the user
    /// never wrote, and left them no way to say what they actually wanted done —
    /// the pane just started talking. Leaving it unsent turns a hijacked turn
    /// into a prefilled one: the user appends their real first instruction and
    /// sends both together, so the agent learns its role and its task at once.
    /// Dispatch still submits, because there no human is at the keyboard.
    pub fn set_conductor(&self, name: Option<String>) {
        *self.conductor.lock().unwrap() = name.clone();
        let _ = self.app.emit("conductor-changed", ());

        let Some(target) = name else { return };
        // A Shell pane has no MCP connection, so it cannot dispatch and the
        // briefing would land in PowerShell as a command.
        let is_agent = self
            .sessions_snapshot()
            .iter()
            .any(|s| s.name == target && crate::is_agent_cli(&s.kind));
        if !is_agent {
            return;
        }
        let peers: Vec<String> = self
            .roster_lines()
            .into_iter()
            .filter(|l| !l.starts_with(&format!("- {target} ")))
            .collect();
        // write_to rather than submit_to: no Enter is sent, which also means no
        // sleep, so this no longer needs a thread of its own.
        let _ = self.engine.write_to(&target, &conductor_briefing(&peers));
    }

    pub fn conductor(&self) -> Option<String> {
        self.conductor.lock().unwrap().clone()
    }

    /// Stop halts all dispatch immediately; clearing it also refreshes the budget.
    pub fn set_halted(&self, v: bool) {
        *self.halted.lock().unwrap() = v;
        if v {
            let mut changed = Vec::new();
            for t in self.tasks.lock().unwrap().iter_mut() {
                if t.status == "pending" {
                    t.status = "cancelled".to_string();
                    changed.push(t.clone());
                }
            }
            let dir = self.dir.lock().unwrap().clone();
            for task in changed {
                let _ = append_record(&dir, &StoreRecord::Task(task));
            }
        } else {
            *self.dispatches.lock().unwrap() = 0;
        }
        let _ = self.app.emit("conductor-changed", ());
    }

    pub fn is_halted(&self) -> bool {
        *self.halted.lock().unwrap()
    }

    pub fn tasks_snapshot(&self) -> Vec<Task> {
        self.tasks.lock().unwrap().clone()
    }

    /// Consume one unit of dispatch budget; false when exhausted.
    fn take_dispatch_budget(&self) -> bool {
        let mut n = self.dispatches.lock().unwrap();
        if *n >= MAX_DISPATCHES {
            return false;
        }
        *n += 1;
        true
    }

    fn add_task(&self, t: Task) {
        let dir = self.dir.lock().unwrap().clone();
        let _ = append_record(&dir, &StoreRecord::Task(t.clone()));
        self.tasks.lock().unwrap().push(t);
        let _ = self.app.emit("conductor-changed", ());
    }

    fn finish_task(&self, caller: &str, id: &str, result: &str) -> Result<(), TaskAccessError> {
        let found = finish_pending(&mut self.tasks.lock().unwrap(), caller, id, result);
        if found.is_ok() {
            if let Some(task) = self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned()
            {
                let dir = self.dir.lock().unwrap().clone();
                let _ = append_record(&dir, &StoreRecord::Task(task));
            }
            let _ = self.app.emit("conductor-changed", ());
        }
        found
    }

    /// Mark a recorded task as errored when terminal delivery fails. A task
    /// that completed or was cancelled concurrently is never overwritten.
    fn mark_delivery_failed(&self, id: &str) {
        let changed = mark_pending_error(&mut self.tasks.lock().unwrap(), id);
        if changed {
            if let Some(task) = self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned()
            {
                let dir = self.dir.lock().unwrap().clone();
                let _ = append_record(&dir, &StoreRecord::Task(task));
            }
            let _ = self.app.emit("conductor-changed", ());
        }
    }

    /// Look a task up, flipping it to "overdue" if it has aged out.
    fn task_status(&self, caller: &str, id: &str) -> Result<Task, TaskAccessError> {
        let mut tasks = self.tasks.lock().unwrap();
        let now = Self::now_ms();
        let t = task_for_dispatcher(&mut tasks, caller, id)?;
        let previous = t.status.clone();
        age(t, now);
        if t.status != previous {
            let dir = self.dir.lock().unwrap().clone();
            let _ = append_record(&dir, &StoreRecord::Task(t.clone()));
        }
        Ok(t.clone())
    }

    /// Every task a given agent dispatched, aged out the same way `task_status`
    /// ages a single one. This is what makes a parallel fan-out cheap to
    /// collect: without it a conductor holding six task ids has to make six
    /// round trips to find out that five are still running.
    fn tasks_from(&self, from: &str) -> Vec<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        let now = Self::now_ms();
        let mut changed = Vec::new();
        let result = tasks
            .iter_mut()
            .filter(|t| t.from == from)
            .map(|t| {
                let previous = t.status.clone();
                age(t, now);
                if t.status != previous {
                    changed.push(t.clone());
                }
                t.clone()
            })
            .collect();
        drop(tasks);
        let dir = self.dir.lock().unwrap().clone();
        for task in changed {
            let _ = append_record(&dir, &StoreRecord::Task(task));
        }
        result
    }

    /// Validate and hand a task to a live session: the shared core of the
    /// agent-facing `dispatch` MCP tool and the human-facing `human_dispatch`
    /// Tauri command (lib.rs) — one ledger and one set of rules regardless of
    /// who started the task. Conductor-only enforcement is deliberately NOT
    /// here: that's an MCP-specific policy the `dispatch` tool applies before
    /// calling this, and `human_dispatch` has no agent identity to check it
    /// against — the human already decided who to promote.
    ///
    /// The task record is created in "pending" status BEFORE `submit_to` is
    /// called. That closes the original race while still allowing an unusually
    /// fast target to complete the task as soon as it receives the prompt.
    pub fn dispatch_task(
        &self,
        from: &str,
        target: &str,
        task: &str,
    ) -> Result<DispatchOutcome, String> {
        let target_is_live = self.engine.ids().iter().any(|i| i == target);
        dispatch_precheck(self.is_halted(), from, target, target_is_live)?;
        if !self.take_dispatch_budget() {
            return Err("dispatch budget exhausted for this run.".to_string());
        }

        let id = uuid::Uuid::new_v4().simple().to_string();
        self.add_task(Task {
            id: id.clone(),
            from: from.to_string(),
            target: target.to_string(),
            task: task.to_string(),
            status: "pending".to_string(),
            result: String::new(),
            ts_ms: Self::now_ms(),
        });

        // Typed into the target's terminal, so the human sees every
        // instruction. Submit Enter separately: Codex and Claude Code treat
        // text+CR in one PTY write as a paste and can leave it waiting in the
        // input editor — see `SessionManager::submit_to`.
        let injection = dispatch_prompt(from, &id, task);
        let delivered = self.engine.submit_to(target, &injection);
        if !delivered {
            self.mark_delivery_failed(&id);
        }
        Ok(DispatchOutcome {
            task_id: id,
            delivered,
        })
    }
}

/// Age a single task in place: still-pending past the threshold becomes
/// "overdue". Shared by `task_status` (one id) and `tasks_from` (a whole
/// list), which used to duplicate this check inline.
///
/// "overdue" is deliberately not terminal. The agent is still running and its
/// result is still accepted, so this only marks the task as slow.
fn age(t: &mut Task, now: u64) {
    if t.status == "pending" && now.saturating_sub(t.ts_ms) > TASK_OVERDUE_MS {
        t.status = "overdue".to_string();
    }
}

/// States a dispatched agent may still report a result from.
///
/// "overdue" belongs here and its absence was a real bug: a task that aged out
/// could never be completed, so an agent that did the whole job came back to
/// report and was refused, and the work was discarded. Nothing ever cancelled
/// that agent, so the wall clock alone decided the result was worthless.
fn accepts_result(status: &str) -> bool {
    matches!(status, "pending" | "overdue")
}

/// The core of `finish_task`: flip a task to "done" if it is still awaiting a
/// result. Pure over a task list, with no lock or emit, so the state
/// transition can be tested directly.
#[derive(Debug, PartialEq)]
enum TaskAccessError {
    NotFound,
    Forbidden,
    NotPending,
}

fn finish_pending(
    tasks: &mut [Task],
    caller: &str,
    id: &str,
    result: &str,
) -> Result<(), TaskAccessError> {
    match tasks.iter_mut().find(|t| t.id == id) {
        Some(t) if t.target != caller => Err(TaskAccessError::Forbidden),
        // Genuinely terminal states still refuse: a cancelled task should not
        // resurrect, and a completed one should not be silently rewritten.
        Some(t) if !accepts_result(&t.status) => Err(TaskAccessError::NotPending),
        Some(t) => {
            t.status = "done".to_string();
            t.result = result.to_string();
            Ok(())
        }
        None => Err(TaskAccessError::NotFound),
    }
}

fn task_for_dispatcher<'a>(
    tasks: &'a mut [Task],
    caller: &str,
    id: &str,
) -> Result<&'a mut Task, TaskAccessError> {
    let task = tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or(TaskAccessError::NotFound)?;
    if task.from != caller {
        return Err(TaskAccessError::Forbidden);
    }
    Ok(task)
}

fn validate_path_component(label: &str, value: &str) -> Result<(), String> {
    let safe = !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if safe {
        Ok(())
    } else {
        Err(format!(
            "invalid {label}: use only ASCII letters, digits, '-' and '_'"
        ))
    }
}

/// Mark delivery failure only while the task is still pending. Completion or
/// cancellation that wins the race remains authoritative.
fn mark_pending_error(tasks: &mut [Task], id: &str) -> bool {
    match tasks
        .iter_mut()
        .find(|t| t.id == id && t.status == "pending")
    {
        Some(t) => {
            t.status = "error".to_string();
            true
        }
        None => false,
    }
}

/// One task rendered for an agent. Kept in one place so a single-id lookup and
/// the collect-everything listing never drift apart.
fn render_task(t: &Task) -> String {
    if t.status == "done" {
        format!("[done] {} → {}\n{}", t.target, t.task, t.result)
    } else {
        format!("[{}] {} → {}", t.status, t.target, t.task)
    }
}

/// How much of the original brief to echo back in a multi-task listing.
const TASK_ECHO_CHARS: usize = 160;

/// Terminal tasks kept in the default listing, newest first.
const RECENT_FINISHED: usize = 10;

fn is_open(status: &str) -> bool {
    // "overdue" is explicitly still running, so it belongs with pending.
    status == "pending" || status == "overdue"
}

fn truncate_chars(s: &str, limit: usize) -> String {
    let mut out: String = s.chars().take(limit).collect();
    if s.chars().nth(limit).is_some() {
        out.push_str("...");
    }
    out
}

/// One task in a listing.
///
/// Deliberately does NOT echo the whole brief. The conductor wrote it and
/// still has it; replaying every prompt back is what grew this response to
/// 111k characters over 39 tasks, past the tool response limit, so the
/// documented way to collect a fan-out failed exactly when a workspace had
/// been used enough to need it. The result is kept whole, because that is the
/// part the caller does not already have.
fn render_task_summary(t: &Task) -> String {
    let brief = truncate_chars(&t.task, TASK_ECHO_CHARS);
    if t.status == "done" {
        format!("[done] {} → {}\n{}", t.target, brief, t.result)
    } else {
        format!("[{}] {} → {}", t.status, t.target, brief)
    }
}

/// The tasks a listing should show, and how many were held back.
///
/// Open tasks are never dropped: "what am I still waiting on" is the question
/// this tool exists to answer, and truncating that would be worse than the
/// size problem it fixes.
fn select_tasks(mine: Vec<Task>, include_all: bool, status: &str) -> (Vec<Task>, usize) {
    if !status.is_empty() {
        let filtered: Vec<Task> = mine.into_iter().filter(|t| t.status == status).collect();
        return (filtered, 0);
    }
    if include_all {
        return (mine, 0);
    }
    let total = mine.len();
    let (open, finished): (Vec<Task>, Vec<Task>) =
        mine.into_iter().partition(|t| is_open(&t.status));
    let kept_finished = finished.len().min(RECENT_FINISHED);
    let dropped = finished.len() - kept_finished;
    // Newest finished first, then re-joined after the open ones.
    let mut recent: Vec<Task> = finished;
    recent.sort_by_key(|t| std::cmp::Reverse(t.ts_ms));
    recent.truncate(kept_finished);
    let mut out = open;
    out.extend(recent);
    debug_assert!(out.len() + dropped == total);
    (out, dropped)
}

fn append_line(path: &PathBuf, s: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(s.as_bytes())
}

/// One agent's MCP handler. Shares the global store; holds its own declared identity.
#[derive(Clone)]
pub struct BrainHandler {
    shared: Arc<Shared>,
    identity: Arc<Mutex<Option<AgentSession>>>,
    /// Set when this handler serves ONE specific session on its own endpoint.
    /// Identity then comes from the connection — it can't be forgotten or spoofed,
    /// so the agent never has to declare a name.
    bound: Option<String>,
    // Used by the #[tool_handler]-generated code, which dead-code analysis can't see.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl BrainHandler {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            identity: Arc::new(Mutex::new(None)),
            bound: None,
            tool_router: Self::tool_router(),
        }
    }

    /// A handler dedicated to one session — used by that session's own endpoint.
    pub fn bound_to(shared: Arc<Shared>, session: String) -> Self {
        Self {
            shared,
            identity: Arc::new(Mutex::new(None)),
            bound: Some(session),
            tool_router: Self::tool_router(),
        }
    }

    fn author(&self) -> String {
        if let Some(b) = &self.bound {
            return b.clone();
        }
        self.identity
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct Identify {
    /// A short name you go by, e.g. "claude-frontend".
    pub name: String,
    /// Your tool/kind, e.g. "claude", "codex", "opencode".
    #[serde(default)]
    pub kind: String,
    /// Optional brain to join. Usually the app assigns this; leave empty to keep
    /// whatever the app set (defaults to "main").
    #[serde(default)]
    pub room: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DecisionArgs {
    /// What the decision is about, e.g. "auth" or "db-schema".
    pub topic: String,
    /// The decision itself.
    pub decision: String,
    /// Optional reasoning other agents should know.
    #[serde(default)]
    pub rationale: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct FactArgs {
    pub category: String,
    pub fact: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct BroadcastArgs {
    pub message: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub query: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DispatchArgs {
    /// The session to hand the task to — use an id from list_sessions.
    pub target: String,
    /// The task, written the way you'd say it to a teammate.
    pub task: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompleteArgs {
    /// The task_id you were given when the work was dispatched to you.
    pub task_id: String,
    /// What you did / what you found.
    pub result: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TaskQuery {
    /// A single task_id to check. Leave this out to collect the fan-out you
    /// are waiting on, which is the efficient way to gather parallel work.
    #[serde(default)]
    pub task_id: String,
    /// Only tasks with this status: pending, overdue, done, error or
    /// cancelled. Use "pending" or "overdue" to ask what is still running.
    #[serde(default)]
    pub status: String,
    /// Include the whole dispatch history rather than open tasks plus the
    /// most recent finished ones. Large workspaces can exceed the response
    /// limit; prefer `status` when you want something specific.
    #[serde(default)]
    pub include_all: bool,
}

#[tool_router]
impl BrainHandler {
    #[tool(
        description = "Declare who you are in this Mosaic workspace. Call once at startup before other tools."
    )]
    fn set_session_identity(&self, Parameters(p): Parameters<Identify>) -> String {
        // On a dedicated endpoint Mosaic already knows who you are.
        if let Some(b) = &self.bound {
            if !p.room.is_empty() {
                if let Err(e) = self.shared.try_set_room(b, &p.room) {
                    return format!("Refused: {e}.");
                }
            }
            return format!("Already identified as '{b}' — Mosaic knows this session.");
        }
        let session = AgentSession {
            name: p.name.clone(),
            kind: p.kind.clone(),
        };
        *self.identity.lock().unwrap() = Some(session.clone());
        // Replace rather than append: an agent that identifies twice should not
        // show up twice in the session list.
        {
            let mut all = self.shared.sessions.lock().unwrap();
            all.retain(|a| a.name != session.name);
            all.push(session);
        }
        if !p.room.is_empty() {
            if let Err(e) = self.shared.try_set_room(&p.name, &p.room) {
                return format!("Refused: {e}.");
            }
        }
        let _ = self.shared.app.emit("context-changed", ());
        format!(
            "Identity set to '{}' in brain '{}'",
            p.name,
            self.shared.room_for(&p.name)
        )
    }

    #[tool(
        description = "Record a decision so every other agent instantly knows it. Use for choices that affect shared work."
    )]
    fn record_decision(&self, Parameters(p): Parameters<DecisionArgs>) -> String {
        let body = if p.rationale.is_empty() {
            p.decision
        } else {
            format!("{} — {}", p.decision, p.rationale)
        };
        self.shared.add("decision", &self.author(), &p.topic, &body);
        "Decision recorded to the shared brain.".to_string()
    }

    #[tool(
        description = "Record a durable fact other agents can rely on (e.g. an API shape, a path, a convention)."
    )]
    fn record_fact(&self, Parameters(p): Parameters<FactArgs>) -> String {
        self.shared
            .add("fact", &self.author(), &p.category, &p.fact);
        "Fact recorded to the shared brain.".to_string()
    }

    #[tool(description = "Broadcast a short message or blocker to all agents.")]
    fn broadcast(&self, Parameters(p): Parameters<BroadcastArgs>) -> String {
        self.shared
            .add("broadcast", &self.author(), "broadcast", &p.message);
        "Broadcast sent.".to_string()
    }

    #[tool(
        description = "Read the shared context (recent decisions, facts, broadcasts) from all agents. Read this before re-deriving something."
    )]
    fn get_shared_context(&self) -> String {
        let room = self.shared.room_for(&self.author());
        let entries = self.shared.entries_snapshot();
        let mine: Vec<&Entry> = entries.iter().filter(|e| e.room == room).collect();
        if mine.is_empty() {
            return format!("No shared context yet in brain '{room}'.");
        }
        let mut out = format!("# Shared context — brain '{room}' (most recent first)\n");
        for e in mine.iter().rev().take(50) {
            out.push_str(&format!(
                "- [{}] ({}) {}: {}\n",
                e.kind, e.author, e.topic, e.body
            ));
        }
        out
    }

    #[tool(description = "Search the shared context for entries containing a query string.")]
    fn search_context(&self, Parameters(p): Parameters<SearchArgs>) -> String {
        let q = p.query.to_lowercase();
        let room = self.shared.room_for(&self.author());
        let entries = self.shared.entries_snapshot();
        let hits: Vec<&Entry> = entries
            .iter()
            .filter(|e| {
                e.room == room
                    && (e.body.to_lowercase().contains(&q) || e.topic.to_lowercase().contains(&q))
            })
            .collect();
        if hits.is_empty() {
            return format!("No matches for '{}'.", p.query);
        }
        let mut out = String::new();
        for e in hits.iter().take(50) {
            out.push_str(&format!("- [{}] {}: {}\n", e.kind, e.topic, e.body));
        }
        out
    }

    #[tool(
        description = "List the other AI agents live in this workspace right now, with their model/CLI and brain. Call this when you are planning work: if you are the conductor these are real, idle agents you can hand tasks to in parallel via dispatch."
    )]
    fn list_sessions(&self) -> String {
        let lines = self.shared.roster_lines();
        if lines.is_empty() {
            return "No live sessions.".to_string();
        }
        let me = self.author();
        let mut out = String::from("# Live sessions\n");
        for l in &lines {
            out.push_str(l);
            out.push('\n');
        }
        // The roster alone reads as passive status. Close with what the caller
        // can actually do with it, which differs by role.
        let peers = lines.len().saturating_sub(1);
        if self.shared.conductor().as_deref() == Some(me.as_str()) {
            out.push_str(&format!(
                "\nYou are the conductor: you can dispatch work to any of the other {peers} session(s). \
                 Dispatch to several before polling, so their work overlaps rather than queues.\n"
            ));
        } else {
            out.push_str(
                "\nYou are not the conductor, so dispatch will refuse — that is expected. \
                 You can still reach these agents through record_decision, record_fact and broadcast.\n",
            );
        }
        out
    }

    #[tool(
        description = "Conductor only: hand a task to another live AI agent in this workspace. Returns immediately with a task_id — it does NOT block — so dispatch every independent piece of work first and collect afterwards with get_task_result, and the agents run in parallel. Reach for this before doing a separable chunk of work yourself: each target is a different model with its own context window. Write the task as you would brief a colleague who cannot see your screen: the goal, the paths involved, and what to report back."
    )]
    fn dispatch(&self, Parameters(p): Parameters<DispatchArgs>) -> String {
        let me = self.author();

        // Conductor-only is MCP-specific policy, so it's checked here rather
        // than in `dispatch_task` — see that method's doc comment.
        if self.shared.is_halted() {
            return "Refused: dispatch is halted by the user (Stop). Do not retry.".to_string();
        }
        match self.shared.conductor() {
            Some(c) if c == me => {}
            Some(_) => {
                return "Refused: you are not the conductor of this workspace.".to_string();
            }
            None => {
                return "Refused: no conductor is set. Ask the user to promote a pane.".to_string()
            }
        }

        match self.shared.dispatch_task(&me, &p.target, &p.task) {
            Err(e) => format!("Refused: {e}"),
            Ok(o) if o.delivered => format!(
                "Dispatched to {} as task {}. Poll get_task_result with that id.",
                p.target, o.task_id
            ),
            Ok(o) => format!(
                "Task {} recorded for {} but delivery failed (could not write to that session) — status is 'error'.",
                o.task_id, p.target
            ),
        }
    }

    #[tool(description = "Report the result of a task the conductor dispatched to you.")]
    fn complete_task(&self, Parameters(p): Parameters<CompleteArgs>) -> String {
        match self
            .shared
            .finish_task(&self.author(), &p.task_id, &p.result)
        {
            Ok(()) => "Result recorded — the conductor can now read it.".to_string(),
            Err(TaskAccessError::Forbidden) => {
                "Refused: this task is assigned to a different session.".to_string()
            }
            Err(TaskAccessError::NotFound) => format!("No task '{}'.", p.task_id),
            Err(TaskAccessError::NotPending) => format!(
                "Task '{}' is already finished or was cancelled, so no result was recorded.",
                p.task_id
            ),
        }
    }

    #[tool(
        description = "Collect the results of work you dispatched. Call with no task_id to get every open task plus the most recently finished ones at once — do that instead of polling ids one by one. Briefs are abbreviated in that listing; pass a task_id for one task in full, status to filter (pending, overdue, done, error, cancelled), or include_all for the whole history. Statuses: an overdue task is STILL RUNNING and its result is still accepted, it has just taken longer than expected, so keep waiting rather than treating it as failed or re-dispatching it."
    )]
    fn get_task_result(&self, Parameters(p): Parameters<TaskQuery>) -> String {
        if !p.task_id.is_empty() {
            return match self.shared.task_status(&self.author(), &p.task_id) {
                Ok(t) => render_task(&t),
                Err(TaskAccessError::Forbidden) => {
                    "Refused: this task was dispatched by a different session.".to_string()
                }
                Err(TaskAccessError::NotFound) => format!("No task '{}'.", p.task_id),
                // `task_status` looks a task up without caring about its state,
                // so this arm is unreachable today. Answering instead of
                // `unreachable!()` keeps a future change to that lookup from
                // turning a lookup into a panic inside a live tool call.
                Err(TaskAccessError::NotPending) => format!("No task '{}'.", p.task_id),
            };
        }

        let mine = self.shared.tasks_from(&self.author());
        if mine.is_empty() {
            return "You have not dispatched any tasks.".to_string();
        }
        let total = mine.len();
        let running = mine.iter().filter(|t| is_open(&t.status)).count();
        let (shown, dropped) = select_tasks(mine, p.include_all, &p.status);

        if shown.is_empty() {
            return format!(
                "No tasks with status '{}'. {total} dispatched in all.",
                p.status
            );
        }

        let mut out = if p.status.is_empty() {
            format!("# Your dispatched tasks ({total} total, {running} still running)\n")
        } else {
            format!(
                "# Your dispatched tasks with status '{}' ({} of {total})\n",
                p.status,
                shown.len()
            )
        };
        for t in &shown {
            out.push_str(&format!(
                "\n## {} → {}\n{}\n",
                t.id,
                t.target,
                render_task_summary(t)
            ));
        }
        if dropped > 0 {
            out.push_str(&format!(
                "\n{dropped} older finished task(s) not shown. Pass include_all for the \
                 whole history, or status to filter. Briefs are abbreviated here; pass a \
                 task_id for one task in full.\n"
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{
        age, append_record, bearer_matches, conductor_briefing, dispatch_precheck, dispatch_prompt,
        finish_pending, load_brain, mark_pending_error, mint_session_token, render_task,
        render_task_summary, select_tasks, task_for_dispatcher, validate_path_component,
        AgentSession, Entry, StoreRecord, Task, TaskAccessError, RECENT_FINISHED, TASK_ECHO_CHARS,
        TASK_OVERDUE_MS,
    };

    fn task_at(id: &str, status: &str, ts_ms: u64) -> Task {
        Task {
            id: id.into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "audit the parser".into(),
            status: status.into(),
            result: String::new(),
            ts_ms,
        }
    }

    #[test]
    fn a_listing_never_drops_an_open_task() {
        // "What am I still waiting on" is the question this tool exists to
        // answer, so windowing must never cost an open task.
        let mut tasks: Vec<Task> = (0..RECENT_FINISHED as u64 * 3)
            .map(|i| task_at(&format!("done{i}"), "done", i))
            .collect();
        tasks.push(task_at("open1", "pending", 999));
        tasks.push(task_at("open2", "overdue", 1000));

        let (shown, dropped) = select_tasks(tasks, false, "");

        let ids: Vec<&str> = shown.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"open1"), "pending task was dropped");
        assert!(ids.contains(&"open2"), "overdue task was dropped");
        assert_eq!(shown.len(), 2 + RECENT_FINISHED);
        assert_eq!(dropped, RECENT_FINISHED * 3 - RECENT_FINISHED);
    }

    #[test]
    fn a_listing_keeps_the_newest_finished_tasks() {
        let tasks: Vec<Task> = (0..RECENT_FINISHED as u64 + 5)
            .map(|i| task_at(&format!("t{i}"), "done", i))
            .collect();

        let (shown, dropped) = select_tasks(tasks, false, "");

        assert_eq!(dropped, 5);
        let oldest_kept = shown.iter().map(|t| t.ts_ms).min().unwrap();
        assert_eq!(oldest_kept, 5, "kept the oldest instead of the newest");
    }

    #[test]
    fn include_all_and_status_bypass_the_window() {
        let tasks: Vec<Task> = (0..RECENT_FINISHED as u64 + 5)
            .map(|i| task_at(&format!("t{i}"), "done", i))
            .collect();
        let total = tasks.len();

        let (all, dropped) = select_tasks(tasks.clone(), true, "");
        assert_eq!(all.len(), total);
        assert_eq!(dropped, 0);

        let (none, _) = select_tasks(tasks, false, "pending");
        assert!(none.is_empty(), "status filter must not invent matches");
    }

    #[test]
    fn a_listing_abbreviates_the_brief_but_never_the_result() {
        // The conductor wrote the brief and still has it. Replaying every
        // prompt is what pushed this response past the tool's size limit.
        let long_brief = "b".repeat(TASK_ECHO_CHARS * 4);
        let long_result = "r".repeat(4000);
        let mut t = task_at("abc", "done", 0);
        t.task = long_brief.clone();
        t.result = long_result.clone();

        let summary = render_task_summary(&t);

        assert!(!summary.contains(&long_brief), "brief was echoed in full");
        assert!(summary.contains("..."), "truncation was not marked");
        assert!(summary.contains(&long_result), "result must survive whole");
        assert!(
            render_task(&t).contains(&long_brief),
            "single lookup stays full"
        );
    }

    #[test]
    fn dispatch_prompt_is_single_line_and_includes_completion_contract() {
        let prompt = dispatch_prompt("sess-1", "abc123", "audit this\r\nthen report");

        assert!(!prompt.contains(['\r', '\n']));
        assert!(prompt.contains("audit this then report"));
        assert!(prompt.contains("task_id \"abc123\""));
    }

    // Both terminal injections share the same hard constraint: an embedded
    // newline submits the message to the target CLI in fragments, so the agent
    // acts on half a briefing. It matters more for the briefing than for a
    // dispatch, because the briefing is left unsent on purpose — a stray newline
    // would fire it off before the user has added anything.
    #[test]
    fn conductor_briefing_is_single_line_and_names_the_peers() {
        let peers = vec![
            "- sess-2 (codex) brain=main".to_string(),
            "- sess-3 (opencode) brain=main".to_string(),
        ];
        let msg = conductor_briefing(&peers);

        assert!(!msg.contains(['\r', '\n']));
        assert!(msg.contains("sess-2 (codex)"));
        assert!(msg.contains("sess-3 (opencode)"));
        assert!(msg.contains("dispatch"));
    }

    #[test]
    fn conductor_briefing_survives_an_empty_workspace() {
        let msg = conductor_briefing(&[]);

        assert!(!msg.contains(['\r', '\n']));
        assert!(msg.contains("No other sessions are open yet"));
    }

    // The user has to be able to type their own instruction after it, so it has
    // to stay short enough to read at a glance. The detail it used to carry now
    // lives in BRAIN_INSTRUCTIONS, which every agent gets on connect.
    #[test]
    fn conductor_briefing_stays_short_enough_to_type_after() {
        let peers = vec!["- sess-2 (codex) brain=main".to_string()];
        let msg = conductor_briefing(&peers);

        assert!(
            msg.len() < 400,
            "briefing is {} chars; it prefills the composer, so it must stay skimmable",
            msg.len()
        );
        // Trailing space so the user's own text does not run into the last word.
        assert!(msg.ends_with(' '));
    }

    #[test]
    fn render_task_shows_the_result_only_once_done() {
        let base = Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "audit the parser".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        };
        assert!(render_task(&base).starts_with("[pending]"));

        let done = Task {
            status: "done".into(),
            result: "found two bugs".into(),
            ..base
        };
        let rendered = render_task(&done);
        assert!(rendered.starts_with("[done]"));
        assert!(rendered.contains("found two bugs"));
    }

    #[test]
    fn render_task_handles_the_error_status() {
        let base = Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "audit the parser".into(),
            status: "error".into(),
            result: String::new(),
            ts_ms: 0,
        };
        assert!(render_task(&base).starts_with("[error]"));
    }

    // dispatch_precheck: order matters because it decides which message wins
    // when more than one check would refuse, and that order is what the live
    // `dispatch` tool preserves by checking `is_halted` first, outside this
    // function, before ever calling it.

    #[test]
    fn dispatch_precheck_refuses_when_halted() {
        let err = dispatch_precheck(true, "sess-1", "sess-2", true).unwrap_err();
        assert!(err.contains("halted"));
    }

    #[test]
    fn dispatch_precheck_refuses_self_dispatch() {
        let err = dispatch_precheck(false, "sess-1", "sess-1", true).unwrap_err();
        assert!(err.contains("cannot dispatch to yourself"));
    }

    #[test]
    fn dispatch_precheck_refuses_a_target_that_is_not_live() {
        let err = dispatch_precheck(false, "sess-1", "sess-2", false).unwrap_err();
        assert!(err.contains("no live session 'sess-2'"));
    }

    #[test]
    fn dispatch_precheck_passes_a_valid_dispatch() {
        assert!(dispatch_precheck(false, "sess-1", "sess-2", true).is_ok());
    }

    #[test]
    fn finish_pending_completes_a_pending_task() {
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "audit the parser".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        }];
        assert_eq!(
            finish_pending(&mut tasks, "sess-2", "abc123", "found two bugs"),
            Ok(())
        );
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[0].result, "found two bugs");
    }

    #[test]
    fn finish_pending_refuses_an_unknown_id() {
        let mut tasks: Vec<Task> = vec![];
        assert_eq!(
            finish_pending(&mut tasks, "sess-2", "nope", "result"),
            Err(TaskAccessError::NotFound)
        );
    }

    #[test]
    fn complete_task_rejects_a_caller_other_than_the_assigned_target() {
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "audit".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        }];

        assert_eq!(
            finish_pending(&mut tasks, "sess-3", "abc123", "stolen"),
            Err(TaskAccessError::Forbidden)
        );
        assert_eq!(tasks[0].status, "pending");
        assert!(tasks[0].result.is_empty());
    }

    #[test]
    fn get_task_result_rejects_a_caller_other_than_the_dispatcher() {
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "audit".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        }];

        assert_eq!(
            task_for_dispatcher(&mut tasks, "sess-3", "abc123").unwrap_err(),
            TaskAccessError::Forbidden
        );
    }

    #[test]
    fn path_components_reject_room_and_session_traversal() {
        assert!(validate_path_component("room", "main").is_ok());
        assert!(validate_path_component("session_id", "sess-1").is_ok());
        assert!(validate_path_component("room", "../../outside").is_err());
        assert!(validate_path_component("session_id", "..\\outside").is_err());
    }

    #[test]
    fn a_session_endpoint_accepts_only_its_own_bearer_token() {
        let token = mint_session_token();
        assert!(bearer_matches(Some(&format!("Bearer {token}")), &token));

        // Every way a caller can get it wrong.
        assert!(!bearer_matches(None, &token), "missing header");
        assert!(!bearer_matches(Some(""), &token), "empty header");
        assert!(
            !bearer_matches(Some(&token), &token),
            "raw token, no scheme"
        );
        assert!(
            !bearer_matches(Some(&format!("Basic {token}")), &token),
            "wrong scheme"
        );
        assert!(
            !bearer_matches(Some(&format!("bearer {token}")), &token),
            "scheme is case-sensitive per RFC 6750 usage here"
        );
        assert!(
            !bearer_matches(Some(&format!("Bearer {}", mint_session_token())), &token),
            "another session's token"
        );
        assert!(
            !bearer_matches(Some(&format!("Bearer {token}x")), &token),
            "correct prefix, extra byte"
        );
        assert!(
            !bearer_matches(
                Some(&format!("Bearer {}", &token[..token.len() - 1])),
                &token
            ),
            "correct prefix, truncated"
        );
    }

    #[test]
    fn session_tokens_are_long_and_distinct() {
        // Guards against a refactor that returns a constant, an empty string,
        // or something short enough to grind against a loopback port that
        // imposes no rate limit.
        let a = mint_session_token();
        let b = mint_session_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64, "two hyphen-free v4 UUIDs");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn mark_pending_error_records_a_delivery_failure() {
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "t".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        }];
        assert!(mark_pending_error(&mut tasks, "abc123"));
        assert_eq!(tasks[0].status, "error");
    }

    #[test]
    fn mark_pending_error_does_not_overwrite_a_cancelled_task() {
        // Simulates set_halted landing between add_task and delivery failure:
        // by the time delivery resolves the task is already "cancelled", and
        // that must win over the delivery outcome.
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "t".into(),
            status: "cancelled".into(),
            result: String::new(),
            ts_ms: 0,
        }];
        assert!(!mark_pending_error(&mut tasks, "abc123"));
        assert_eq!(tasks[0].status, "cancelled");
    }

    #[test]
    fn age_marks_a_stale_pending_task_overdue_but_leaves_other_statuses_alone() {
        let base = Task {
            id: "a".into(),
            from: "f".into(),
            target: "t".into(),
            task: "x".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        };

        let mut pending = base.clone();
        age(&mut pending, TASK_OVERDUE_MS + 1);
        assert_eq!(pending.status, "overdue");

        let mut errored = Task {
            status: "error".into(),
            ..base
        };
        age(&mut errored, TASK_OVERDUE_MS + 1);
        assert_eq!(errored.status, "error");
    }

    #[test]
    fn an_overdue_task_still_accepts_its_result() {
        // The bug this fixes. Nothing cancels a dispatched agent, so one that
        // ran past the threshold kept working, finished the job, called
        // complete_task, and was refused. The work was done and thrown away
        // because a wall clock had moved.
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "long research task".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 0,
        }];

        age(&mut tasks[0], TASK_OVERDUE_MS + 1);
        assert_eq!(tasks[0].status, "overdue");

        assert_eq!(
            finish_pending(&mut tasks, "sess-2", "abc123", "here is the report"),
            Ok(())
        );
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[0].result, "here is the report");
    }

    #[test]
    fn genuinely_terminal_states_still_refuse_a_result() {
        // The tolerance must not resurrect a cancelled task or silently
        // rewrite one that already reported.
        for status in ["cancelled", "done", "error"] {
            let mut tasks = vec![Task {
                id: "abc123".into(),
                from: "sess-1".into(),
                target: "sess-2".into(),
                task: "t".into(),
                status: status.into(),
                result: "original".into(),
                ts_ms: 0,
            }];
            assert_eq!(
                finish_pending(&mut tasks, "sess-2", "abc123", "late overwrite"),
                Err(TaskAccessError::NotPending),
                "{status} must not accept a result"
            );
            assert_eq!(tasks[0].result, "original");
        }
    }

    #[test]
    fn an_overdue_task_still_checks_the_caller() {
        // Accepting late results must not weaken authorization.
        let mut tasks = vec![Task {
            id: "abc123".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "t".into(),
            status: "overdue".into(),
            result: String::new(),
            ts_ms: 0,
        }];
        assert_eq!(
            finish_pending(&mut tasks, "sess-3", "abc123", "stolen"),
            Err(TaskAccessError::Forbidden)
        );
        assert_eq!(tasks[0].status, "overdue");
    }

    #[test]
    fn durable_brain_round_trips_and_applies_latest_task_state() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_path_buf();
        let entry = Entry {
            kind: "decision".into(),
            author: "sess-1".into(),
            topic: "storage".into(),
            body: "use jsonl".into(),
            ts_ms: 1,
            room: "main".into(),
        };
        let session = AgentSession {
            name: "sess-1".into(),
            kind: "codex".into(),
        };
        let pending = Task {
            id: "task-1".into(),
            from: "sess-1".into(),
            target: "sess-2".into(),
            task: "test persistence".into(),
            status: "pending".into(),
            result: String::new(),
            ts_ms: 2,
        };
        let done = Task {
            status: "done".into(),
            result: "passed".into(),
            ..pending.clone()
        };

        append_record(&dir, &StoreRecord::Entry(entry.clone())).unwrap();
        append_record(&dir, &StoreRecord::Session(session.clone())).unwrap();
        append_record(&dir, &StoreRecord::Task(pending)).unwrap();
        append_record(&dir, &StoreRecord::Task(done.clone())).unwrap();

        let loaded = load_brain(&dir);
        assert_eq!(loaded.entries, vec![entry]);
        assert_eq!(loaded.sessions, vec![session]);
        assert_eq!(loaded.tasks, vec![done]);
    }

    #[test]
    fn durable_brain_ignores_corrupt_and_partial_lines() {
        use std::io::Write;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_path_buf();
        let entry = Entry {
            kind: "fact".into(),
            author: "sess-1".into(),
            topic: "safe".into(),
            body: "valid records survive".into(),
            ts_ms: 1,
            room: "main".into(),
        };
        append_record(&dir, &StoreRecord::Entry(entry.clone())).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(super::STORE_FILE))
            .unwrap();
        file.write_all(b"not json\n{\"type\":\"entry\",\"value\":")
            .unwrap();

        let loaded = load_brain(&dir);
        assert_eq!(loaded.entries, vec![entry]);
        assert!(loaded.sessions.is_empty());
        assert!(loaded.tasks.is_empty());
        assert!(load_brain(&temp.path().join("missing")).entries.is_empty());
    }
}

/// What every connecting agent is told about the workspace it just joined.
///
/// Exposing well-described tools is not enough on its own: an agent that is not
/// told to consult shared context simply won't, and the brain stays empty while
/// two agents build incompatible halves of the same thing. MCP carries these
/// instructions on the connection itself, which is why this lives here rather
/// than in each project's AGENTS.md — it reaches every session automatically,
/// in whichever repo it was launched against.
///
/// The workspace section exists for a second, distinct failure: an agent that
/// treats Mosaic as a nicer terminal and never notices the other panes are
/// usable capacity. Because MCP delivers this text once, at connect time, it can
/// only describe the role an agent *might* be given — the conductor briefing
/// injected by `set_conductor` is what covers the role it actually has.
const BRAIN_INSTRUCTIONS: &str = r#"You are one of several AI agents working in parallel inside Mosaic, each in its own terminal, on the same project at the same time. This server is your shared brain: it is how you learn what the others have already decided, how they learn what you decide, and how work is handed between you.

Mosaic already knows who you are from this connection. You do not need to call set_session_identity.

## The workspace

The other panes are not logs or history. They are live AI coding agents — often different models, each with its own separate context window — sitting idle until given work. Call list_sessions to see who is here.

Mosaic gives exactly one session the conductor role, and the user assigns it; you cannot claim it. Call list_sessions to find out whether that is you, and expect the answer to change during a run.

If you ARE the conductor, the rest of the workspace is yours to direct, and using it is the point of this tool:
- Before doing a separable piece of work yourself, ask whether it should be dispatched instead. Independent slices — different files or subsystems, separate research questions, a second opinion from a different model — are what the other sessions are for.
- dispatch returns immediately with a task_id rather than blocking. So dispatch every independent task first and collect afterwards; that is what makes the agents run in parallel instead of queueing behind each other.
- Call get_task_result with no task_id to collect every task you dispatched in one call, rather than polling ids one at a time.
- A dispatched agent cannot see your screen or your context. State the goal, the concrete paths, and what you want reported back.
- This does not replace your own subagents. Prefer a Mosaic session when you want a different model or a genuinely separate context window; prefer your own subagents for work inside your own.

If you are NOT the conductor, dispatch will refuse — that is expected, not an error to work around. When a line starting with "[mosaic] Task from conductor" appears in your terminal, that is real work assigned to you: carry it out, then call complete_task with the task_id you were given and a summary of the result. The conductor is waiting on that call.

## Shared context

- BEFORE making a decision that affects shared work — architecture, dependencies, data models, API shapes, file layout, naming conventions — call get_shared_context. Another agent may have already settled it. Do not re-derive or quietly contradict an existing decision; if you disagree with one, broadcast the disagreement instead of diverging in silence.
- Use search_context to check one specific topic before you spend effort researching it.
- AFTER making such a decision, call record_decision with the topic, the decision, and your reasoning. This is the single most important thing you do here — it is what stops two agents building halves that don't fit together. If you are dispatching work that depends on a convention, record it before you dispatch.
- Use record_fact for durable things others will need: an API shape, a path, a command, a convention you just established.
- Use broadcast for blockers, or anything the others need to know immediately."#;

#[tool_handler]
impl ServerHandler for BrainHandler {
    // Supplying get_info suppresses the macro's generated one, so both the tools
    // capability and our own name/version have to be restated here — otherwise
    // no tools are advertised, and the server introduces itself to agents as
    // "rmcp" (the default is resolved inside that crate, not ours).
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mosaic", env!("CARGO_PKG_VERSION")))
            .with_instructions(BRAIN_INSTRUCTIONS)
    }
}

/// Bind the MCP server on a random loopback port and spawn it. Returns the port
/// and the shared store (also used by the frontend `get_context` command).
/// Bind a loopback endpoint dedicated to ONE session. Because only that session
/// is registered against this port, every request on it is provably from that
/// session — identity without a handshake.
/// The caller owns the returned handle: dropping it does NOT stop the server, so
/// it must be aborted explicitly when the session ends, or the listener outlives
/// the session it was bound to.
pub struct SessionServer {
    pub port: u16,
    /// Secret this session must present as `Authorization: Bearer <token>`.
    /// Handed to the caller so it can be written into that one session's agent
    /// config; it is never logged and never leaves the machine.
    pub token: String,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl SessionServer {
    /// Stop serving. Called when the session is killed or exits on its own.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// Mint a secret for one session's endpoint.
///
/// Two v4 UUIDs, whose randomness comes from the OS CSPRNG via `getrandom`,
/// give 244 bits with no new dependency. Far past brute force, which matters
/// because the endpoint sits on loopback where anything local can reach it and
/// retry without limit.
fn mint_session_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Whether an `Authorization` header carries exactly the expected bearer token.
///
/// Compared in constant time over the whole candidate so a caller cannot learn
/// the secret one byte at a time from response latency. Localhost makes that
/// attack awkward rather than impossible, and the cost of not leaking is a
/// single XOR per byte. Split out from the middleware so it is directly
/// testable without standing up a server.
fn bearer_matches(header: Option<&str>, expected: &str) -> bool {
    let Some(value) = header else {
        return false;
    };
    let Some(presented) = value.strip_prefix("Bearer ") else {
        return false;
    };
    if presented.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in presented.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub fn start_session_server(
    shared: Arc<Shared>,
    session_id: String,
) -> std::io::Result<SessionServer> {
    validate_path_component("session_id", &session_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let token = mint_session_token();
    let expected = token.clone();

    let task = tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(_) => return,
        };
        let service = StreamableHttpService::new(
            move || Ok(BrainHandler::bound_to(shared.clone(), session_id.clone())),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        // The port still identifies WHICH session this is; the token proves the
        // caller is that session rather than any other local process that
        // guessed the port. Both checks, not one instead of the other.
        let router =
            axum::Router::new()
                .nest_service("/mcp", service)
                .layer(axum::middleware::from_fn(
                    move |req: axum::extract::Request, next: axum::middleware::Next| {
                        let expected = expected.clone();
                        async move {
                            let header = req
                                .headers()
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok());
                            if bearer_matches(header, &expected) {
                                next.run(req).await
                            } else {
                                axum::http::StatusCode::UNAUTHORIZED.into_response()
                            }
                        }
                    },
                ));
        let _ = axum::serve(listener, router).await;
    });

    Ok(SessionServer { port, token, task })
}

pub fn start(
    app: AppHandle,
    dir: PathBuf,
    engine: Arc<crate::SessionManager>,
) -> std::io::Result<(u16, Arc<Shared>)> {
    let brain = load_brain(&dir);
    let shared = Arc::new(Shared {
        app,
        dir: Mutex::new(dir),
        entries: Mutex::new(brain.entries),
        sessions: Mutex::new(brain.sessions),
        name_to_room: Mutex::new(HashMap::new()),
        engine,
        conductor: Mutex::new(None),
        halted: Mutex::new(false),
        tasks: Mutex::new(brain.tasks),
        dispatches: Mutex::new(0),
    });

    // Bind synchronously so we can hand the port back before the server task runs.
    let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();

    let shared_for_server = shared.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(_) => return,
        };
        let service = StreamableHttpService::new(
            move || Ok(BrainHandler::new(shared_for_server.clone())),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        let router = axum::Router::new().nest_service("/mcp", service);
        let _ = axum::serve(listener, router).await;
    });

    Ok((port, shared))
}
