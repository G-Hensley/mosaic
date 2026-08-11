# Backlog

Ideas are not commitments. Promote an item only once its trigger, ownership,
security boundary, and validation method are understood.

`IMPROVEMENT-AUDIT.md` holds findings from a point-in-time review of existing
code. This file holds capabilities Mosaic does not have yet.

---

## Liveness: tell a slow agent apart from a dead one

Found while testing the overdue fix, and partly caused by it.

Before, a task past the threshold flipped to "timeout" and its result was
refused. That was wrong, and it is fixed. But the fix traded one failure for
another: a task is now **never** terminal on its own. If the agent process dies,
its task sits at "overdue" forever and the conductor waits on a result that can
never arrive.

Observed directly: three OpenCode panes, only two `opencode` processes alive,
and the third pane's task stuck at "overdue" with `complete_task` never called.
Nothing in Mosaic noticed the process was gone.

"overdue" is honest about not knowing, which is better than a false "timeout".
But Mosaic does know something it is not using: it spawned the process and can
see whether it is still running.

- Mark a task `abandoned` when its target session's process is gone. That is a
  real terminal state, distinct from "cancelled" (deliberate) and from
  "overdue" (still working).
- Surface pane liveness in `list_sessions`, so a conductor does not dispatch
  into a dead pane in the first place.
- Consider whether a dead pane's task should be re-dispatchable to another
  session, and whether that should be automatic or offered.

## `get_task_result` with no id outgrows its own response limit

It returns every task ever dispatched. At 28 tasks that is already ~67k
characters, which exceeds the tool response limit and fails outright, so the
documented way to collect a fan-out breaks exactly when a workspace has been
used for a while.

Wants a default window: open tasks plus recently finished ones, with older
history behind an explicit flag. Filtering by status would also let a conductor
ask the question it actually has, which is "what am I still waiting on".

## Dispatch can lose the head of a long prompt

A Codex session received a dispatch that began mid-sentence, at
`d guard: a zero spending limit...`. The preamble and the first two numbered
questions were simply absent, and the agent answered a truncated brief while
correctly flagging that it had done so.

A follow-up test with a length-matched prompt arrived complete, token and both
sentinel phrases intact, so this is **not** a simple length limit and not tail
truncation. Leading bytes are being dropped somewhere between `write_to` and the
target CLI's input buffer.

The truncated dispatch was one of four sent in immediate succession; the clean
one was sent alone to an idle session. Contention during a fan-out is the
obvious hypothesis and the obvious thing to test first. Note this is distinct
from the known submit race in `IMPROVEMENT-AUDIT.md` #1, which drops the Enter
rather than the text.

Silent corruption is worse than a failed dispatch: the agent does plausible work
on the wrong brief. Whatever the cause, dispatch should verify what landed, or
fail loudly.

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
