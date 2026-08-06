// Mosaic "shared brain": an in-process MCP server on loopback that every agent
// CLI connects to. Agents publish decisions/facts/broadcasts and read the shared
// context, so one agent's decision instantly becomes another's knowledge. The
// tool handlers touch app state directly (same process), and each write emits a
// `context-changed` event so the sidebar updates live.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
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

#[derive(Clone, Serialize)]
pub struct Entry {
    pub kind: String, // "decision" | "fact" | "broadcast"
    pub author: String,
    pub topic: String,
    pub body: String,
    pub ts_ms: u64,
    pub room: String, // which brain this belongs to
}

#[derive(Clone, Serialize)]
pub struct AgentSession {
    pub name: String,
    pub kind: String,
}

/// One dispatched unit of work, from the conductor to another session.
#[derive(Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub from: String,
    pub target: String,
    pub task: String,
    /// "pending" | "done" | "timeout" | "cancelled"
    pub status: String,
    pub result: String,
    pub ts_ms: u64,
}

/// Guardrails. Note that depth is bounded structurally: only the conductor may
/// dispatch, so a dispatched agent cannot dispatch onward.
const MAX_DISPATCHES: u32 = 40;
const TASK_TIMEOUT_MS: u64 = 10 * 60 * 1000;

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

    /// Re-point the markdown mirror (the user picked a different project).
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock().unwrap() = dir;
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
    pub fn set_room(&self, name: &str, room: &str) {
        self.name_to_room
            .lock()
            .unwrap()
            .insert(name.to_string(), room.to_string());
        let _ = self.app.emit("context-changed", ());
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
            s.push(AgentSession {
                name: name.to_string(),
                kind: kind.to_string(),
            });
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
            for t in self.tasks.lock().unwrap().iter_mut() {
                if t.status == "pending" {
                    t.status = "cancelled".to_string();
                }
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
        self.tasks.lock().unwrap().push(t);
        let _ = self.app.emit("conductor-changed", ());
    }

    fn finish_task(&self, id: &str, result: &str) -> bool {
        let found = {
            let mut tasks = self.tasks.lock().unwrap();
            match tasks.iter_mut().find(|t| t.id == id && t.status == "pending") {
                Some(t) => {
                    t.status = "done".to_string();
                    t.result = result.to_string();
                    true
                }
                None => false,
            }
        };
        if found {
            let _ = self.app.emit("conductor-changed", ());
        }
        found
    }

    /// Look a task up, flipping it to "timeout" if it has aged out.
    fn task_status(&self, id: &str) -> Option<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        let now = Self::now_ms();
        let t = tasks.iter_mut().find(|t| t.id == id)?;
        if t.status == "pending" && now.saturating_sub(t.ts_ms) > TASK_TIMEOUT_MS {
            t.status = "timeout".to_string();
        }
        Some(t.clone())
    }

    /// Every task a given agent dispatched, aged out the same way `task_status`
    /// ages a single one. This is what makes a parallel fan-out cheap to
    /// collect: without it a conductor holding six task ids has to make six
    /// round trips to find out that five are still running.
    fn tasks_from(&self, from: &str) -> Vec<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        let now = Self::now_ms();
        tasks
            .iter_mut()
            .filter(|t| t.from == from)
            .map(|t| {
                if t.status == "pending" && now.saturating_sub(t.ts_ms) > TASK_TIMEOUT_MS {
                    t.status = "timeout".to_string();
                }
                t.clone()
            })
            .collect()
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

fn append_line(path: &PathBuf, s: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
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
    /// A single task_id to check. Leave this out to get every task you have
    /// dispatched, which is the efficient way to collect a parallel fan-out.
    #[serde(default)]
    pub task_id: String,
}

#[tool_router]
impl BrainHandler {
    #[tool(description = "Declare who you are in this Mosaic workspace. Call once at startup before other tools.")]
    fn set_session_identity(&self, Parameters(p): Parameters<Identify>) -> String {
        // On a dedicated endpoint Mosaic already knows who you are.
        if let Some(b) = &self.bound {
            if !p.room.is_empty() {
                self.shared.set_room(b, &p.room);
            }
            return format!("Already identified as '{b}' — Mosaic knows this session.");
        }
        let session = AgentSession { name: p.name.clone(), kind: p.kind.clone() };
        *self.identity.lock().unwrap() = Some(session.clone());
        // Replace rather than append: an agent that identifies twice should not
        // show up twice in the session list.
        {
            let mut all = self.shared.sessions.lock().unwrap();
            all.retain(|a| a.name != session.name);
            all.push(session);
        }
        if !p.room.is_empty() {
            self.shared.set_room(&p.name, &p.room);
        }
        let _ = self.shared.app.emit("context-changed", ());
        format!("Identity set to '{}' in brain '{}'", p.name, self.shared.room_for(&p.name))
    }

    #[tool(description = "Record a decision so every other agent instantly knows it. Use for choices that affect shared work.")]
    fn record_decision(&self, Parameters(p): Parameters<DecisionArgs>) -> String {
        let body = if p.rationale.is_empty() {
            p.decision
        } else {
            format!("{} — {}", p.decision, p.rationale)
        };
        self.shared.add("decision", &self.author(), &p.topic, &body);
        "Decision recorded to the shared brain.".to_string()
    }

    #[tool(description = "Record a durable fact other agents can rely on (e.g. an API shape, a path, a convention).")]
    fn record_fact(&self, Parameters(p): Parameters<FactArgs>) -> String {
        self.shared.add("fact", &self.author(), &p.category, &p.fact);
        "Fact recorded to the shared brain.".to_string()
    }

    #[tool(description = "Broadcast a short message or blocker to all agents.")]
    fn broadcast(&self, Parameters(p): Parameters<BroadcastArgs>) -> String {
        self.shared.add("broadcast", &self.author(), "broadcast", &p.message);
        "Broadcast sent.".to_string()
    }

    #[tool(description = "Read the shared context (recent decisions, facts, broadcasts) from all agents. Read this before re-deriving something.")]
    fn get_shared_context(&self) -> String {
        let room = self.shared.room_for(&self.author());
        let entries = self.shared.entries_snapshot();
        let mine: Vec<&Entry> = entries.iter().filter(|e| e.room == room).collect();
        if mine.is_empty() {
            return format!("No shared context yet in brain '{room}'.");
        }
        let mut out = format!("# Shared context — brain '{room}' (most recent first)\n");
        for e in mine.iter().rev().take(50) {
            out.push_str(&format!("- [{}] ({}) {}: {}\n", e.kind, e.author, e.topic, e.body));
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

        if self.shared.is_halted() {
            return "Refused: dispatch is halted by the user (Stop). Do not retry.".to_string();
        }
        match self.shared.conductor() {
            Some(c) if c == me => {}
            Some(_) => {
                return "Refused: you are not the conductor of this workspace.".to_string();
            }
            None => return "Refused: no conductor is set. Ask the user to promote a pane.".to_string(),
        }
        if p.target == me {
            return "Refused: cannot dispatch to yourself.".to_string();
        }
        if !self.shared.engine.ids().iter().any(|i| i == &p.target) {
            return format!(
                "Refused: no live session '{}'. Call list_sessions for valid targets.",
                p.target
            );
        }
        if !self.shared.take_dispatch_budget() {
            return "Refused: dispatch budget exhausted for this run.".to_string();
        }

        let uid = uuid::Uuid::new_v4().simple().to_string();
        let id = uid[..6].to_string();
        // Typed into the target's terminal, so the human sees every instruction.
        // Submit Enter separately: Codex and Claude Code treat text+CR in one
        // PTY write as a paste and can leave it waiting in the input editor.
        let injection = dispatch_prompt(&me, &id, &p.task);
        if !self.shared.engine.submit_to(&p.target, &injection) {
            return "Refused: could not write to that session.".to_string();
        }

        self.shared.add_task(Task {
            id: id.clone(),
            from: me,
            target: p.target.clone(),
            task: p.task.clone(),
            status: "pending".to_string(),
            result: String::new(),
            ts_ms: Shared::now_ms(),
        });
        format!(
            "Dispatched to {} as task {}. Poll get_task_result with that id.",
            p.target, id
        )
    }

    #[tool(description = "Report the result of a task the conductor dispatched to you.")]
    fn complete_task(&self, Parameters(p): Parameters<CompleteArgs>) -> String {
        if self.shared.finish_task(&p.task_id, &p.result) {
            "Result recorded — the conductor can now read it.".to_string()
        } else {
            format!("No pending task '{}'.", p.task_id)
        }
    }

    #[tool(
        description = "Collect the results of work you dispatched. Call with no task_id to get every task you dispatched at once — do that instead of polling ids one by one. Statuses are pending, done (with the result), timeout or cancelled."
    )]
    fn get_task_result(&self, Parameters(p): Parameters<TaskQuery>) -> String {
        if !p.task_id.is_empty() {
            return match self.shared.task_status(&p.task_id) {
                None => format!("No task '{}'.", p.task_id),
                Some(t) => render_task(&t),
            };
        }

        let mine = self.shared.tasks_from(&self.author());
        if mine.is_empty() {
            return "You have not dispatched any tasks.".to_string();
        }
        let pending = mine.iter().filter(|t| t.status == "pending").count();
        let mut out = format!(
            "# Your dispatched tasks ({} total, {} still running)\n",
            mine.len(),
            pending
        );
        for t in &mine {
            out.push_str(&format!("\n## {} → {}\n{}\n", t.id, t.target, render_task(t)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{conductor_briefing, dispatch_prompt, render_task, Task};

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
pub fn start_session_server(shared: Arc<Shared>, session_id: String) -> std::io::Result<u16> {
    let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(_) => return,
        };
        let service = StreamableHttpService::new(
            move || Ok(BrainHandler::bound_to(shared.clone(), session_id.clone())),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        let router = axum::Router::new().nest_service("/mcp", service);
        let _ = axum::serve(listener, router).await;
    });

    Ok(port)
}

pub fn start(
    app: AppHandle,
    dir: PathBuf,
    engine: Arc<crate::SessionManager>,
) -> std::io::Result<(u16, Arc<Shared>)> {
    let shared = Arc::new(Shared {
        app,
        dir: Mutex::new(dir),
        entries: Mutex::new(Vec::new()),
        sessions: Mutex::new(Vec::new()),
        name_to_room: Mutex::new(HashMap::new()),
        engine,
        conductor: Mutex::new(None),
        halted: Mutex::new(false),
        tasks: Mutex::new(Vec::new()),
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
