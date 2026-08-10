# Backlog

Ideas are not commitments. Promote an item only once its trigger, ownership,
security boundary, and validation method are understood.

`IMPROVEMENT-AUDIT.md` holds findings from a point-in-time review of existing
code. This file holds capabilities Mosaic does not have yet.

---

## Model-aware dispatch

Today the conductor knows a session's id and CLI (`sess-3 (opencode)`) and
nothing about what that session is *good at*. So routing is guesswork, and the
guesses have been wrong in practice: broad web research kept going to OpenCode
sessions on a free-tier model, which is close to the worst available match for
it.

`Projects/knowledge/ai-kbase/MODEL-GUIDE.md` already contains the missing
knowledge, maintained and dated, including a task-to-model routing table, per
CLI strengths, cost and quota strategy, and the OpenRouter free pool.

**The shape to aim for:** the conductor learns each pane's model and a short
capability profile, so `list_sessions` answers "who should do this" rather than
only "who is here".

Open questions before building:

- **Do not vendor a copy.** A snapshot of `MODEL-GUIDE.md` inside Mosaic is
  stale the day it is written, and the guide already carries `last_verified`
  and a 14-day cadence. Read it from a configured path, or import it through
  the agent-toolkit catalog, rather than duplicating it.
- The guide's routing table is human-shaped prose. Deciding what a
  machine-readable profile needs (strengths, context window, cost tier, tool
  support, latency expectation) is most of the work.
- What happens when the guide is absent, since not every machine will have it.
  Degrade to today's behaviour rather than failing.

## Orchestrator may open the sessions the work needs

Currently the human opens panes with Ctrl+K and the conductor dispatches to
whatever exists. The conductor can see that a task wants a different model and
can do nothing about it.

Worth exploring: let the conductor request a new session of a named kind, so a
plan that needs a second opinion from a different vendor can arrange one.

This is a privilege escalation and needs treating as such:

- Spawning a session starts a real process, may create a git worktree, and
  consumes quota. `MAX_DISPATCHES` exists for a reason; an equivalent ceiling
  is needed here.
- Decide whether it is a request the human approves or an autonomous action.
  Approval by default is the safer starting point.
- A conductor that can create sessions can create them in a loop. Bound it
  structurally, the way dispatch depth is already bounded by only the conductor
  being able to dispatch.
- Interaction with `SESSION-RESTORE-PROPOSAL.md`: restored panes and
  conductor-created panes should not fight over ids.

## Guardrail: OpenCode sessions must stay on free OpenRouter models

OpenRouter is configured with a real account, so an OpenCode pane can select a
paid model and silently spend money. Nothing in Mosaic currently constrains
this, and the conductor cannot see what a pane costs.

The naive implementation is wrong in a specific way worth writing down:

**Do not detect "free" by checking that prompt and completion pricing are
zero.** `MODEL-GUIDE.md` records that a model can price prompt and completion at
zero while still charging per request, per generated image, or per audio clip.
The documented signal is the `:free` suffix on the model id, plus the
`openrouter/free` meta-id which selects from the live free pool.

Also account for:

- **Free quota is account-wide**, not per session: 20 requests/minute, and 50
  requests/day until $10 of lifetime credit has been purchased, 1,000/day after.
  A fan-out across several OpenCode panes shares one budget and can exhaust the
  daily allowance quickly. An orchestrator that spawns sessions needs to know
  this before it spawns them.
- **`openrouter/free` does not promise a stable model identity.** It selects a
  compatible model per request, so two calls can land on different models. Pin
  an explicit `:free` id where reproducibility matters.
- **Free is explicitly not a reliability tier.** The guide notes free
  availability and latency vary. This is almost certainly why dispatches to
  OpenCode panes ran long often enough to expose the discarded-result bug fixed
  in `fix/late-task-completion`; the two issues share a root cause in tier
  choice.
- **It is still hosted inference.** Free does not mean private. Do not route
  work over private code or credentials to this tier without checking the
  selected provider's data policy.

Enforcement point is undecided and matters: Mosaic can only realistically
constrain what it launches, so this may belong in OpenCode's own config
(generated from `.agents/`, per the toolkit's sync) rather than in Mosaic. If
Mosaic enforces it, it needs a way to observe the model actually in use, which
it does not have today.
