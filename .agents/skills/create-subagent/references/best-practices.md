# Subagent Best Practices

Apply these rules across hosts, then use the host reference for syntax and inheritance.

## One bounded responsibility

Give the agent one job with a clear handoff. State the nearest responsibilities it does not own. A reviewer should review; an implementer should implement; a researcher should gather evidence.

Avoid arbitrary line limits. Prefer the shortest prompt that still defines:

- the outcome;
- boundaries;
- required evidence;
- verification;
- return format.

## Capability plus routing conditions

The description should say what the agent provides and when the parent should delegate to it.

```text
Weak: Helps with code.
Good: Review code changes for correctness, security, and missing tests. Use for pull-request, branch, patch, and diff reviews.
```

Keep procedural steps out of the description.

## Least privilege

- Give research and review roles read-only effective permissions.
- Enable writes or commands only when the role requires them.
- Treat prompt instructions and tool lists as behavioral controls, not security boundaries.
- Verify the effective sandbox and permission policy because parent settings may take precedence.
- Scope MCP access to the role rather than exposing every configured server.

## Inheritance

Inheritance differs by host and version.

- **Codex:** custom agent files are configuration layers. Omitted model, sandbox, MCP, and skill settings can inherit from parent/default configuration. Current documentation does not support a blanket claim that `AGENTS.md` is never available.
- **Claude Code:** tools are inherited unless restricted; permission modes have precedence rules; the custom agent prompt replaces the default Claude Code system prompt.
- **Agy:** runtime subagents and plugin agent components are documented, but the public standalone manifest schema is not currently documented.

Keep durable project conventions in the host's project-guidance mechanism. Repeat only role-critical constraints, and test effective context when correctness depends on inheritance.

## Evidence and return contract

Tell the subagent what the parent needs back. Prefer:

- file and line references;
- commands run and relevant output;
- changed files and verification results;
- uncertainty and unresolved risks;
- prioritized findings with concrete failure modes.

Avoid pass/fail grades and generic confidence scores. They conceal actionable evidence.

## Model selection

Inherit the parent model by default. Override it only when the role has a stable need for a different cost, latency, context, or reasoning profile. Verify model identifiers against the installed host instead of copying examples indefinitely.

## Validation

Run a harmless representative task after creating an agent. Confirm discovery, routing, effective permissions, tool availability, expected output, and failure behavior. Record the host version used for schema-sensitive validation outside the manifest.
