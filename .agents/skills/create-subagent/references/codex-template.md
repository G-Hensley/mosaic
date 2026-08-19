# Codex Custom-Agent Template

Codex loads standalone TOML agent files from `.codex/agents/` for a project and `~/.codex/agents/` for a user. The current required fields are `name`, `description`, and `developer_instructions`.

## Minimal template

```toml
name = '<agent-name>'
description = '<Capability and concrete conditions for delegating work to this agent.>'

developer_instructions = '''
# Role

Own one bounded responsibility.

## Boundaries

- Do: <permitted work>.
- Do not: <nearby work owned by another agent>.

## Workflow

1. Inspect the evidence supplied by the parent.
2. Perform the focused analysis or implementation.
3. Verify the result using the allowed environment.

## Return

Report findings or changes with file references, evidence, and unresolved uncertainty.
'''
```

The `name` field is authoritative; matching the filename is the clearest convention.

## Optional settings

A custom-agent file is a Codex configuration layer. It may use supported `config.toml` settings such as:

```toml
model = '<installed-model-id>'
model_reasoning_effort = '<effort-supported-by-that-model>'
sandbox_mode = 'read-only'
```

Omit model and effort unless this agent needs a different tier. Codex resolves omitted model settings through explicit spawn values, global subagent defaults, and then the parent. Other omitted session settings—including sandbox, MCP servers, and skill configuration—inherit from the parent.

Do not add undocumented fields such as `nickname_candidates` to a reusable template.

## Workspace guidance

Current Codex documentation does not establish the old blanket claim that custom agents never receive `AGENTS.md`. Do not encode either inheritance assumption as fact.

- Keep durable project conventions in the applicable `AGENTS.md`.
- Put agent-specific role boundaries in `developer_instructions`.
- Repeat a project constraint only when this specialist cannot safely operate without it.
- Verify effective behavior in the installed Codex version when inheritance is material.

## Review example

```toml
name = 'reviewer'
description = 'Review code changes for correctness, security, and missing tests. Use for pull-request, branch, patch, and diff reviews.'
sandbox_mode = 'read-only'

developer_instructions = '''
# Code Reviewer

Review the supplied changes. Do not edit files or create commits.

Prioritize defects that can change behavior, expose data, weaken security, or leave important paths untested. Read surrounding code before judging a diff in isolation.

Return findings ordered by severity. For each finding, include the file and line, the concrete failure mode, and the smallest defensible correction. End with residual risks and verification gaps. Do not assign a pass/fail grade.
'''
```

## Verification

After saving the file:

1. Start a new Codex session if discovery does not refresh.
2. Ask Codex to delegate a harmless representative task to the named agent.
3. Inspect the subagent thread and effective sandbox.
4. Confirm any configured model exists locally and accepts the selected effort.

Source: current Codex manual, Custom agents section.
