# Claude Code Subagent Template

Claude Code stores project agents in `.claude/agents/<name>.md` and user agents in `~/.claude/agents/<name>.md`. Files use YAML frontmatter followed by the agent prompt.

## Minimal template

```markdown
---
name: <agent-name>
description: <Capability and concrete conditions for delegating work to this agent.>
model: inherit
tools:
  - Read
  - Grep
  - Glob
permissionMode: plan
---

# Role

Own one bounded responsibility.

## Boundaries

- Do: <permitted work>.
- Do not: <nearby work owned by another agent>.

## Workflow

1. Inspect the supplied evidence and relevant surrounding files.
2. Perform the focused task.
3. Verify the conclusion.

## Return

Report findings with file references, evidence, and unresolved uncertainty.
```

Omit optional fields when inheritance is appropriate.

## Current fields to know

| Field | Purpose |
|---|---|
| `name` | Agent identifier; use kebab-case and match the filename |
| `description` | Capability and routing conditions |
| `model` | `inherit`, a supported alias such as `sonnet`, `opus`, or `haiku`, or a full model ID |
| `tools` | Allowlist; omitted means inherit the parent's available tools |
| `disallowedTools` | Remove selected tools while inheriting the rest |
| `permissionMode` | Permission behavior such as `default`, `dontAsk`, or read-only `plan`; parent modes can take precedence |
| `skills` | Preload selected skills into the agent context |
| `mcpServers` | Attach or reference MCP servers for a local agent definition |
| `memory` | Persistent per-agent scope: `user`, `project`, or `local` |

Claude Code supports additional fields including `effort`, `maxTurns`, `background`, and `isolation`. Add them only for a concrete requirement and verify the installed version.

Memory locations are `~/.claude/agent-memory/<name>/` for `user`,
`.claude/agent-memory/<name>/` for `project`, and
`.claude/agent-memory-local/<name>/` for `local`. Prefer `project` for shareable
project learning. See [memory-template.md](memory-template.md) for portable policy
and promotion rules.

Tool restriction reduces capability but is not a complete security boundary. Use the effective permission mode and sandbox controls as well.

## Review example

```markdown
---
name: code-reviewer
description: Review code changes for correctness, safety, and missing tests. Use for pull-request, branch, patch, and diff reviews.
model: inherit
tools:
  - Read
  - Grep
  - Glob
permissionMode: plan
---

# Code Reviewer

Review the supplied changes. Do not edit files or create commits.

Prioritize defects that can change behavior, expose data, weaken security, or leave important paths untested. Read surrounding code before judging a diff in isolation.

Return findings ordered by severity. For each finding, include the file and line, the concrete failure mode, and the smallest defensible correction. End with residual risks and verification gaps. Do not assign a pass/fail grade.
```

## Verification

1. Open `/agents` and confirm the definition appears.
2. Run a harmless representative task.
3. Inspect effective tools and permissions; parent permission modes may override the file.
4. Confirm referenced skills and MCP servers are available.

Source: Claude Code documentation, Create custom subagents.
