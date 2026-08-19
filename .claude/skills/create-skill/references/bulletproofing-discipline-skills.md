# Bulletproofing Discipline Skills

Use this reference **only when writing discipline skills** — skills whose job is to hold under pressure when the agent doesn't want to follow them. Examples: TDD, validation-before-completion, no-force-push, no-secrets-in-code, dry-run-before-destructive-ops.

For other skill types (technique, pattern, reference), this is overkill — they're solved by clarity, not by anti-rationalization armor.

---

## Why Discipline Skills Are Different

A technique skill ("how to do X") fails when the agent doesn't understand the technique. The fix is clarity.

A discipline skill ("don't skip Y, even when…") fails when the agent rationalizes their way around the rule. The fix is **explicitly closing every loophole the agent might find**.

Smart agents under pressure are creative. The skill must out-anticipate them.

---

## Pattern 1: "Violating The Letter Is Violating The Spirit"

State this principle early in the skill body. It cuts off an entire class of "I'm following the spirit" rationalizations before they begin.

```markdown
**Violating the letter of the rules is violating the spirit of the rules.**
```

Without this, the agent will reason: "The rule says X, but the *intent* of the rule was Y, so I can do Z because Z still serves Y." The principle preempts that reasoning.

---

## Pattern 2: Close Every Loophole Explicitly

Don't just state the rule — forbid specific workarounds you've seen agents try.

**Weak:**
```markdown
Write code before test? Delete it.
```

**Strong:**
```markdown
Write code before test? Delete it. Start over.

**No exceptions:**
- Don't keep it as "reference"
- Don't "adapt" it while writing tests
- Don't look at it for inspiration
- Don't comment it out and re-enable later
- Delete means delete
```

Each bullet was an actual rationalization an agent offered in baseline testing. Each one needs an explicit "no."

---

## Pattern 3: Rationalization Table

Capture rationalizations from baseline testing (see `./testing-skills.md`) and put them directly in the skill body. The agent reading the skill sees their own future excuses pre-empted.

```markdown
| Excuse | Reality |
|---|---|
| "Too simple to test" | Simple code breaks. Test takes 30 seconds. |
| "I'll test after" | Tests passing immediately prove nothing. |
| "Tests-after achieve same goals" | Tests-after = "what does this do?" Tests-first = "what should this do?" |
| "It's about spirit not ritual" | **Violating the letter is violating the spirit.** |
| "This case is different because…" | The rule has no exceptions. |
| "Senior eng said it's fine" | Authority doesn't override the rule. Push back or escalate. |

**All of these mean: follow the rule.**
```

The "all of these mean: …" line collapses the rationalizations into the single forced action. No matter which excuse the agent generates, the answer is the same.

---

## Pattern 4: Red Flags List

A self-check list that helps the agent recognize they're *about* to rationalize.

```markdown
## Red Flags — STOP And Reconsider

- "I already manually tested it"
- "Tests after achieve the same purpose"
- "It's about spirit not ritual"
- "This case is different because…"
- "We're behind schedule and this is the easy path"
- "Senior eng said it's okay"

**All of these mean: STOP. Follow the rule.**
```

Same trick as the rationalization table — collapses many excuses into one forced action.

---

## Pattern 5: "No Exceptions" Sections

For the absolute rules, put them in a dedicated section with that exact heading. Future Claude reading the skill can't miss them.

```markdown
## No Exceptions

The Iron Law applies in every case:

- Not for "simple additions"
- Not for "documentation updates"
- Not for "I'll fix it after"
- Not under deadline pressure
- Not because senior eng said so
- Not because the test is "obviously going to pass"
```

---

## Pattern 6: Description Reinforces The Trigger, Not The Workflow

For discipline skills especially, the description must focus on triggering conditions — when the rule applies — not what the skill says to do. See `./description-discipline.md`.

```yaml
# BAD — workflow summary; agent skips skill body
description: Use for TDD — write test first, watch it fail, write minimal code, refactor

# GOOD — triggering condition
description: Use when implementing any feature or bugfix, before writing implementation code
```

If the description summarizes the workflow, the agent will follow the description and skip the body. The body is where the bulletproofing lives.

---

## Pattern 7: The Iron Law

State the rule once, in capital letters, as a single line that the reader can quote back. Examples:

```markdown
NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST.

NEVER --force OR --force-with-lease ON git OPERATIONS WITHOUT EXPLICIT INSTRUCTION.

NO COMMIT BEFORE LINT, TYPE-CHECK, AND TESTS PASS.
```

Iron Laws are short, absolute, and quotable. They're the line the agent shouldn't cross even under maximum pressure.

---

## Pattern 8: Authority Stand-In

Discipline skills work better when they invoke authority the agent recognizes — the operator's documented preference, an established practice, or a referenced incident. Examples:

- "The operator's CLAUDE.md states: TDD is not optional."
- "Last quarter's outage was caused by skipping this exact step."
- "This pattern was established after two incidents documented in `docs/postmortems/`."

The agent is more likely to follow a rule they recognize as someone else's commitment than one that feels arbitrary.

(For the underlying psychology — Cialdini's authority/commitment principles — academic background not required to apply the pattern.)

---

## When NOT To Use These Patterns

For non-discipline skills:

- **Technique skills** — clear instructions and one good example beat anti-rationalization armor
- **Pattern skills** — when-to-use criteria and counter-examples are what's needed, not "no exceptions" lists
- **Reference skills** — searchability and accuracy matter; bulletproofing is noise

Bulletproofing applied to a reference doc reads as preachy and discourages use. Reserve it for skills where rationalization is the failure mode.

---

## Cross-References

- `./testing-skills.md` — TDD-for-skills cycle that produces the rationalization data this reference is built from
- `./description-discipline.md` — descriptions that summarize workflow defeat bulletproofing by giving the agent a way to skip the body
- `./anti-patterns.md` — content patterns that fail in tested skills
- `./skill-template.md` — discipline-skill template variant
