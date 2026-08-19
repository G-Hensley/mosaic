---
name: create-skill
description: Create or revise portable Agent Skills with correct structure, triggering metadata, supporting resources, and validation. Use when a user asks to create, scaffold, improve, or audit a SKILL.md-based skill.
---

# Create Skill

Build a portable skill around the open Agent Skills format. Keep the portable core independent of Claude-, Codex-, Agy-, or OpenCode-only behavior.

## Process

### 1. Establish the use cases

Collect or infer:

- the concrete requests that should trigger the skill;
- the requests that should not trigger it;
- the required output or behavior;
- any scripts, references, or assets that should be reusable;
- the intended installation scope and hosts.

Ask only when a missing choice would materially change the result.

### 2. Check existing skills

- Search the target skills root for overlapping names and capabilities.
- Extend an existing skill when the new material shares the same trigger and workflow.
- Create a separate skill when it has a distinct trigger, responsibility, or security boundary.

### 3. Choose the name and location

- Use lowercase letters, digits, and hyphens; keep the name under 64 characters.
- Prefer a short verb-led name such as `create-subagent` or `review-migrations`.
- Match the folder name to the `name` field exactly.
- Install the finished skill as `<skills-root>/<name>/SKILL.md`. Do not place another discoverable skill inside this skill's folder.

Common portable roots include `.agents/skills/` for a project and `~/.agents/skills/` for a user. Host-specific mirrors or adapters are distribution concerns, not part of the portable core.

### 4. Write portable frontmatter

Use only the open-format fields in the portable `SKILL.md`:

```yaml
---
name: example-skill
description: Perform a specific capability. Use when the request includes concrete triggering conditions or artifacts.
---
```

The description must state both what the skill provides and when it applies. Put workflow details in the body. Read [description-discipline.md](references/description-discipline.md) before finalizing it.

Do not put Claude-only fields such as `disable-model-invocation`, `context`, `agent`, `allowed-tools`, or `paths` into a skill advertised as portable. Put optional host behavior in a host adapter or packaging layer.

Track freshness in repository metadata or an external inventory rather than adding unrecognized top-level frontmatter fields.

### 5. Build the body and resources

- Use imperative instructions and an ordered workflow when sequence matters.
- Include only information an agent cannot reliably infer.
- **Keep the body at or under 200 lines. This is enforced, not advised** — `agent-toolkit validate` fails a skill that exceeds it. The open format permits 500, but the whole body loads on every activation, so length is a recurring context cost, and depth buried in prose is depth an agent skims. Move detail into `references/`, which loads only when needed.
- Put deterministic or repeatedly rewritten logic in `scripts/`.
- Put templates or output material in `assets/`.
- Link every supporting resource directly from `SKILL.md` with relative paths.
- Avoid absolute paths and undocumented dynamic-injection syntax in the portable core.

Use [skill-template.md](references/skill-template.md) as the starting point and [anti-patterns.md](references/anti-patterns.md) during review.

### 6. Validate behavior

Choose validation proportional to the skill:

- **Reference:** answer a question that requires the bundled reference.
- **Technique or pattern:** solve a fresh representative problem and verify correct recognition and application.
- **Task or scaffolding:** invoke it against a temporary target and inspect the produced artifacts.
- **Discipline-enforcing:** compare baseline and skill-assisted behavior under realistic pressure; read [bulletproofing-discipline-skills.md](references/bulletproofing-discipline-skills.md).

Use isolated sessions or subagents only when the environment permits them. Do not leak the expected diagnosis into an evaluator. Preserve raw prompts, outputs, diffs, and logs for nontrivial tests. See [testing-skills.md](references/testing-skills.md).

### 7. Verify structure and discovery

- Confirm `SKILL.md` exists and its YAML parses.
- Confirm `name` and `description` are the only portable top-level fields.
- Confirm the folder name matches `name`.
- Confirm every relative resource link resolves.
- Confirm there are no nested `SKILL.md` files inside the skill.
- Inspect the host's skill list or invoke the skill explicitly after installation.
- Test every host the skill claims to support; successful discovery in one host does not prove another host will load it.

## Reference Files

- [skill-template.md](references/skill-template.md) — portable templates and host-adapter boundary.
- [description-discipline.md](references/description-discipline.md) — capability-and-trigger descriptions and naming guidance.
- [testing-skills.md](references/testing-skills.md) — behavioral testing and RED-GREEN-REFACTOR guidance.
- [anti-patterns.md](references/anti-patterns.md) — content, structure, and process mistakes.
- [bulletproofing-discipline-skills.md](references/bulletproofing-discipline-skills.md) — additional pressure testing for discipline-enforcing skills.
