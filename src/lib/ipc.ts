import { Channel, invoke } from "@tauri-apps/api/core";

// Raw channel messages are normally ArrayBuffers, but Tauri historically sent
// small raw messages as number[] (fixed in 2.5.1). Handle every shape.
export type Bytes = ArrayBuffer | ArrayBufferView | number[];

export function toBytes(msg: Bytes): Uint8Array {
  if (msg instanceof ArrayBuffer) return new Uint8Array(msg);
  if (ArrayBuffer.isView(msg)) {
    return new Uint8Array(msg.buffer, msg.byteOffset, msg.byteLength);
  }
  return new Uint8Array(msg);
}

export type SessionType = {
  id: string;
  label: string;
  program: string;
  args: string[];
  color: string;
};

// The launchable session types. The three agent CLIs are wired to the shared
// brain automatically at launch (see agent_mcp_wiring in the backend); Shell is
// a plain terminal with no MCP wiring.
export const SESSION_TYPES: SessionType[] = [
  { id: "shell", label: "Shell", program: "powershell.exe", args: ["-NoLogo"], color: "#7dcfff" },
  { id: "claude", label: "Claude Code", program: "claude", args: [], color: "#e0af68" },
  { id: "codex", label: "Codex", program: "codex", args: [], color: "#bb9af7" },
  { id: "opencode", label: "opencode", program: "opencode", args: [], color: "#9ece6a" },
];

export function spawnSession(
  sessionId: string,
  channel: Channel<Bytes>,
  program: string,
  args: string[],
  rows: number,
  cols: number,
  opts?: { cwd?: string; isolate?: boolean },
): Promise<void> {
  return invoke("spawn_session", {
    sessionId,
    channel,
    program,
    args,
    rows,
    cols,
    cwd: opts?.cwd,
    isolate: opts?.isolate,
  });
}

export const writeSession = (sessionId: string, data: string): Promise<void> =>
  invoke("write_session", { sessionId, data });

export const resizeSession = (sessionId: string, rows: number, cols: number): Promise<void> =>
  invoke("resize_session", { sessionId, rows, cols });

export const killSession = (sessionId: string): Promise<void> =>
  invoke("kill_session", { sessionId });

// ---- Shared brain (MCP) ----
export type ContextEntry = {
  kind: string; // "decision" | "fact" | "broadcast"
  author: string;
  topic: string;
  body: string;
  ts_ms: number;
  room: string; // which brain
};
export type AgentIdentity = { name: string; kind: string };
export type ContextSnapshot = { entries: ContextEntry[]; sessions: AgentIdentity[] };
export type McpInfo = { url: string; port: number };

export const getContext = (): Promise<ContextSnapshot> => invoke("get_context");
export const mcpInfo = (): Promise<McpInfo> => invoke("mcp_info");
export const setAgentBrain = (name: string, brain: string): Promise<void> =>
  invoke("set_agent_brain", { name, brain });

// Tell the backend which project is active, so the shared brain writes its
// markdown into that project's .mosaic/context instead of a global pile.
export const setProject = (path: string | null): Promise<void> =>
  invoke("set_project", { path });

// ---- Conductor ----
export type ConductorTask = {
  id: string;
  from: string;
  target: string;
  task: string;
  // pending | overdue | done | error | cancelled.
  // "overdue" is still running: past the reporting threshold but not
  // cancelled, and its result is still accepted.
  status: string;
  result: string;
  // Stamped once at dispatch and never moved. This is not when the task
  // finished — see done_ms.
  ts_ms: number;
  // When the task reached a terminal state, or null while it is still live
  // (including in_review and rework, neither of which is finished).
  done_ms: number | null;
};
export type ConductorState = {
  conductor: string | null;
  halted: boolean;
  tasks: ConductorTask[];
};

export const conductorState = (): Promise<ConductorState> => invoke("conductor_state");
export const setConductor = (name: string | null): Promise<void> =>
  invoke("set_conductor", { name });
export const haltConductor = (halted: boolean): Promise<void> =>
  invoke("halt_conductor", { halted });

// ---- Dispatch (human-initiated) ----
export const dispatchTask = (target: string, task: string): Promise<{ task_id: string }> =>
  invoke("human_dispatch", { target, task });
