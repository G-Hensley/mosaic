# Agent Memory Policy

Use memory only when accumulated experience will improve the agent's bounded
responsibility. Keep stateless utilities memory-free.

For a toolkit agent package, place this beside `agent.md` as `memory.yaml`:

```yaml
enabled: true
scope: project
promotion: review
categories:
  - patterns
  - pitfalls
  - architecture
  - improvement-candidates
```

Choose `project` for shareable repository learning, `local` for untracked or
sensitive repository learning, and `user` only for expertise that genuinely
applies across projects. Use `promotion: never` when memory must not influence
canonical reusable artifacts.

Claude Code supports a native `memory` frontmatter field. Map the portable scope
directly:

- `user` -> `~/.claude/agent-memory/<agent>/`
- `project` -> `.claude/agent-memory/<agent>/`
- `local` -> `.claude/agent-memory-local/<agent>/`

Claude loads the beginning of the agent's `MEMORY.md` and gives the subagent
tools to maintain its memory when auto memory is enabled. Include concise prompt
instructions to consult memory before work, curate stable findings, and record
evidence for promotion candidates.

Do not emit a memory field for Codex, OpenCode, or Agy until the installed host
has a verified native contract. Retain the portable policy for a future adapter
or an explicitly configured MCP memory service.

Treat memory as fallible working knowledge. Promote a learning into an agent,
skill, policy, or knowledge source only after evidence review and validation.

Source: Claude Code documentation, Create custom subagents, persistent memory
section; verified 2026-08-05 against Claude Code 2.1.223.
