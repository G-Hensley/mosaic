---
name: create-subagent
description: Create or revise focused custom-agent definitions for Claude Code, Codex, OpenCode, or Agy. Use when a user asks for a subagent, custom agent, delegated specialist, reviewer, researcher, or host-specific agent file.
---

# Create Subagent

Create a narrow specialist for the selected host without pretending that agent manifests are portable. The role and behavioral intent may be shared; file formats, inheritance, permissions, and discovery must be handled per host.

## Process

### 1. Establish the role

Determine:

- the task the agent owns and the nearest tasks it must refuse;
- the host or hosts;
- whether it needs read, write, command, web, MCP, or delegation capabilities;
- the desired model only when the default is unsuitable;
- whether it should learn across sessions and whether that learning is
  user-wide, project-shareable, or project-local;
- the evidence and output the parent needs back.

Prefer one bounded responsibility over a general-purpose persona.

### 2. Check conflicts

Search the host's supported project and user locations before creating anything:

- Claude Code: `.claude/agents/<name>.md` or `~/.claude/agents/<name>.md`.
- Codex: `.codex/agents/<name>.toml` or `~/.codex/agents/<name>.toml`.
- OpenCode: `.opencode/agents/<name>.md` or `~/.config/opencode/agents/<name>.md`.
- Agy: the selected plugin's `agents/` component; do not assume Gemini CLI's standalone `.gemini/agents/` convention applies.

If a same-name definition exists, inspect it and extend or replace it only with the user's authorization.

### 3. Read the host reference

- Claude Code: [claude-template.md](references/claude-template.md).
- Codex: [codex-template.md](references/codex-template.md).
- OpenCode: [opencode-template.md](references/opencode-template.md).
- Agy: [agy-template.md](references/agy-template.md).
- Shared design rules: [best-practices.md](references/best-practices.md).
- Portable memory policy: [memory-template.md](references/memory-template.md).

Do not copy fields between hosts merely because their names look similar.

### 4. Write the definition

- Match the documented host schema and location.
- Include required fields only, then add optional restrictions intentionally.
- State the role, boundaries, workflow, evidence requirements, and return format.
- Keep project facts discoverable through the host's normal context mechanism; repeat only constraints that are essential to this specialist's role.
- Do not hardcode a model unless the task requires a different cost, speed, or capability tier.
- Do not claim that a tool list or prompt is a security boundary; use the host's sandbox and permission controls.
- Keep live memory isolated by agent and scope. Never point multiple agents at
  one undifferentiated writable memory directory.
- Record reusable observations as review candidates; do not let raw memory
  rewrite canonical agents or skills automatically.

For Agy, create a packaged custom agent only when a current schema is available from official documentation or a known-good installed plugin. Otherwise use Agy's native runtime subagents and report that the custom manifest schema remains unverified.

### 5. Verify

- Parse YAML or TOML using an appropriate parser.
- Confirm required fields and target paths.
- Confirm referenced tools and models exist in the installed host version.
- Inspect the host's agent panel or list and run a harmless representative task.
- Verify a read-only agent cannot write through the effective sandbox or permission policy.
- Record the host version and source used for schema-sensitive changes outside the agent manifest.

## Reference Files

- [memory-template.md](references/memory-template.md) — portable memory policy, native mappings, and promotion rules.
- [best-practices.md](references/best-practices.md) — role boundaries, inheritance, permissions, and output contracts.
- [claude-template.md](references/claude-template.md) — Claude Code markdown agent format.
- [codex-template.md](references/codex-template.md) — Codex TOML custom-agent format.
- [opencode-template.md](references/opencode-template.md) — OpenCode markdown custom-agent format.
- [agy-template.md](references/agy-template.md) — documented Agy capabilities and the current schema boundary.
