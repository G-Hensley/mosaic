# Agy Subagent Guidance

Verified against Agy CLI 1.1.10 and the official Antigravity CLI feature documentation on 2026-08-05.

## What is documented

Agy has an asynchronous subagent framework. The main agent can delegate background research, tests, and other focused work, and `/agents` opens the panel for inspecting active and completed agents.

Agy plugins can bundle an `agents/` component alongside skills, rules, MCP servers, and hooks. Installed plugins are staged under:

```text
~/.gemini/antigravity-cli/plugins/<plugin-name>/
├── plugin.json
├── agents/
├── skills/
├── rules/
├── mcp_config.json
└── hooks.json
```

The `.gemini` parent in this staging path is an implementation detail retained by Antigravity; the product and executable are Agy, not Gemini CLI.

## What is not documented

The current public Agy documentation does not publish a standalone custom-agent file schema equivalent to Claude's `.claude/agents/*.md` or Codex's `.codex/agents/*.toml` schema.

Therefore:

- Do not generate `.gemini/agents/<name>.md` from the former Gemini CLI template.
- Do not invent YAML frontmatter fields or tool names.
- Do not claim a plugin agent manifest is valid merely because the plugin directory contains `agents/`.
- Do not copy Claude agent files into Agy without a known-good Agy example or schema.

## Safe workflow

1. Prefer Agy's native runtime delegation when the user only needs parallel or background work.
2. Use `/agents` to inspect running subagents and approvals.
3. For a reusable plugin agent, locate a current official schema or an installed known-good plugin from the same Agy version.
4. Copy only fields demonstrated by that source and record the source and Agy version in repository documentation.
5. Stage the result through the supported plugin installation path and verify it appears in `/agents`.
6. If no schema is available, stop before creating the manifest and report the exact uncertainty.

## Model selection

Use `/model` to select Agy's default reasoning model. Do not hardcode a Gemini model in an agent definition unless the verified Agy schema explicitly supports it. Agy is a host and may expose models from more than one provider.

Official source: Antigravity CLI Features at https://www.agy.dev/docs/cli/features
