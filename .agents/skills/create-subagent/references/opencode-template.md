# OpenCode Custom-Agent Template

OpenCode stores project agents in `.opencode/agents/<name>.md` and user agents in
`~/.config/opencode/agents/<name>.md`. The filename is the agent identifier.

## Minimal template

```markdown
---
description: Review code changes for correctness, security, and missing tests.
mode: subagent
permission:
  edit: deny
  bash: ask
---

# Role

Review the supplied changes. Do not edit files.

## Return

Report prioritized findings with file references, evidence, and unresolved risk.
```

## Fields

- `description` controls routing and should include concrete use conditions.
- `mode` is `primary`, `subagent`, or `all`; use `subagent` for specialists.
- `model` uses `provider/model-id`. Omit it to inherit the invoking primary model.
- `permission` controls tool access. Prefer it over the deprecated `tools` field.
- `hidden` removes a subagent from autocomplete but does not make it inaccessible
  to permitted task delegation.

Keep provider-specific model options in the OpenCode adapter rather than the
shared role prompt. Verify installed model identifiers with `opencode models`.

## Verification

1. Run `opencode agent list` from the target project.
2. Invoke the agent with `@<name>` on a harmless representative task.
3. Confirm effective permissions, model inheritance, and return format.

Source: https://opencode.ai/docs/agents/
