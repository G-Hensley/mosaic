# Portable Skill Template

Use this structure for a skill intended to work across Agent Skills hosts.

## Minimal template

```markdown
---
name: <kebab-case-name>
description: <What capability this provides and the concrete situations in which it should be used.>
---

# <Skill Name>

<One concise sentence defining the outcome.>

## Workflow

1. <First required action.>
2. <Conditional or core action.>
3. <Verification and completion criteria.>

## Resources

- [reference.md](references/reference.md) — <when and why to read it>.
- `scripts/helper.ps1` — <when and why to run it>.
```

Create only the resource directories the skill actually needs.

## Frontmatter contract

| Field | Required | Constraint |
|---|---:|---|
| `name` | Yes | Lowercase letters, digits, and hyphens; max 64 characters; matches the parent folder |
| `description` | Yes | States capability and triggering conditions; max 1024 characters |

For the portable core, do not add other top-level fields. Unknown fields may be ignored, rejected, or interpreted differently across hosts.

## Host adapters

Keep host-only behavior outside the portable `SKILL.md`:

| Host | Put optional behavior here |
|---|---|
| Codex / ChatGPT | `agents/openai.yaml` for UI metadata, invocation policy, and dependencies |
| Claude Code | A Claude-specific copy or adapter when fields such as `disable-model-invocation`, `context`, `agent`, `allowed-tools`, or `paths` are required |
| Agy | Plugin packaging when distributing skills, agents, rules, MCP servers, or hooks together |
| OpenCode | OpenCode configuration or permission policy outside the portable skill |

Do not make required behavior depend on an extension that another claimed host ignores.

## Design checks

- Put detailed knowledge in `references/`, deterministic work in `scripts/`, and output templates in `assets/`.
- Link reference files directly from `SKILL.md`; avoid reference chains.
- Prefer one representative example over repeated variants.
- Keep machine-specific paths and secrets out of the skill.
- Track verification dates outside portable frontmatter.
