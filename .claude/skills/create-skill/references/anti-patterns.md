# Skill Anti-Patterns

Patterns that look reasonable when writing skills and fail in practice. If you find yourself doing one of these, stop and revisit.

---

## Description Anti-Patterns

See `./description-discipline.md` for depth. Quick list:

- **Workflow-only description** — it omits the capability or trigger and duplicates body content.
- **First-person description** — `"I help with..."` instead of third-person; the description is injected into the system prompt.
- **Vague trigger** — `"For testing"`; future Claude can't tell if it applies.
- **Missing keyword coverage** — no error messages, symptoms, synonyms, or natural-language phrases the operator would say.

---

## Content Anti-Patterns

### Narrative Examples

```
BAD: "In session 2025-10-03, we found that empty projectDir caused..."
```

Too specific to the moment, not reusable. Future readers don't know what you knew that day. Translate the lesson into a pattern they can apply without context.

### Multi-Language Dilution

```
BAD: example-js.js, example-py.py, example-go.go, example-java.java
```

Five mediocre examples in five languages instead of one excellent example. Maintenance burden compounds; readers see five mediocre examples of something that should have been crisp.

**One excellent example beats many mediocre ones.** Pick the language that fits the problem (testing → TS/JS, system debugging → shell/Python, data → Python). A capable agent can port one good example to other languages.

### Code In Flowcharts

```dot
BAD:
step1 [label="import fs"];
step2 [label="read file"];
```

Can't copy-paste, hard to read, defeats the point of code. Use markdown code blocks for code; use flowcharts only for non-obvious decision points.

### Generic Labels In Diagrams

```
BAD: helper1, helper2, step3, pattern4
GOOD: validate-input, fetch-user, persist-record, emit-event
```

Labels carry the diagram's meaning. Generic labels are noise.

### Fill-In-The-Blank Templates Instead Of Real Examples

```python
# BAD — generic template, not adaptable
def YOUR_FUNCTION(YOUR_ARGS):
    YOUR_LOGIC
    return YOUR_RESULT
```

Readers can't tell what the pattern actually looks like in real code. Show a concrete, runnable, well-commented example.

### Flowcharts For Linear Processes

```dot
BAD: a sequence of steps that's literally just steps
```

Flowcharts are for non-obvious decision points. For "step 1 then step 2 then step 3," use a numbered list.

---

## Structural Anti-Patterns

### Undocumented Host-Specific Loading Syntax

```
BAD: @skills/testing/test-driven-development/SKILL.md
```

Import and command syntax varies by host. An undocumented host-specific loader in a portable skill may eagerly load content, fail silently, or appear as literal text. Use ordinary relative links instead:

```
GOOD: See [testing methodology](./testing-skills.md).
```

Package required material directly or link it so the host can load it on demand.

### Absolute Or Machine-Specific Reference Paths

```
BAD:  [best practices](file:///c:/Users/Alice/proj/.agents/skills/x/references/bp.md)
BAD:  See C:\Users\Alice\proj\references\bp.md
GOOD: See `references/bp.md`.
```

Reference links must be **relative** to SKILL.md (`references/<file>.md` or `./<file>.md`).
Absolute paths — `file:///...`, `C:\...`, `/Users/...` — are hardcoded to one machine and
user: they break on a fresh clone, on any other machine, and for every other tool reading
the shared skill. Skills are portable; their internal links must be too.

### Bloated SKILL.md

Keep SKILL.md at or under **200 lines**, enforced by `agent-toolkit validate`. Split sooner when detail can be loaded selectively:

- Heavy reference material (>100 lines) → its own file
- Reusable templates / scripts → its own file
- Per-language code → grouped reference

The SKILL.md itself stays a process or dispatcher. References hold the depth.

### Nonportable Freshness Metadata

Skills decay, but arbitrary top-level fields are not portable. Track verification dates in repository metadata, release automation, or a host-specific adapter instead of adding `last_verified` to a shared `SKILL.md`.

### Missing Reference Files Section

Even if the skill has only one reference, list it explicitly. Future Claude scans the bottom of SKILL.md for "what else is here." Hidden references = unused references.

### Dropping The Conflict Check

Adding a skill that overlaps with an existing one creates two competing voices. Routing becomes unpredictable. Always verify no existing skill covers the territory before creating a new one.

---

## Process Anti-Patterns

### Ship Untested Discipline Skills

Discipline skills (TDD, validation, no-force-push) exist *specifically* to hold under pressure. Untested means you don't know if they hold. See `./testing-skills.md`.

### Batch Skill Creation Without Testing Each

```
BAD: Write skill A, write skill B, write skill C, then test all three
```

Each skill needs its own RED-GREEN-REFACTOR cycle. Batching skips the per-skill verification and produces a backlog of half-broken skills.

### "I'll Refactor Later" After A Test Failure

Test failed? The skill needs the fix *now*, not later. The rationalization the agent gave is data — fold it into the skill before moving on.

### Editing Without Re-Testing

Edits to discipline skills can introduce loopholes the original test surfaced. Re-test after non-trivial edits to confirm compliance still holds.

---

## Cross-References

- `./description-discipline.md` — description-specific anti-patterns in depth
- `./testing-skills.md` — how to catch these failure modes before deploying
- `./bulletproofing-discipline-skills.md` — discipline-skill-specific anti-patterns and counters
- `./skill-template.md` — frontmatter table, three template variants
