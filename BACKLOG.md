# Backlog

Ideas are not commitments. Promote an item only once its trigger, ownership,
security boundary, and validation method are understood.

`IMPROVEMENT-AUDIT.md` holds findings from a point-in-time review of existing
code. This file holds capabilities Mosaic does not have yet.

---

## Prior art: nobody has solved the OpenCode local-model timeout

Checked before investing further. Short answer: it is a known, unfixed problem,
and the one team that documented it in depth gave up and moved to hosted models.

Note the repo moved: `sst/opencode` now redirects to `anomalyco/opencode`.

| Issue | Substance | Status |
|---|---|---|
| [#29420](https://github.com/anomalyco/opencode/issues/29420) | Names the cause: the timeout mechanism used `AbortSignal.timeout()`, "which does not work correctly in Bun's runtime", so provider requests had no effective timeout. Proposes a stream watchdog with a **30s first-byte** and 120s idle timeout | **Closed as not planned** |
| [#2974](https://github.com/anomalyco/opencode/issues/2974) | Config `timeout` "totally ignored" for local providers. The reporter used 900000, the same value I tried | Closed |
| [#3708](https://github.com/anomalyco/opencode/issues/3708) | Timeouts persist on larger models despite config | Open |
| [#20466](https://github.com/anomalyco/opencode/issues/20466) | "SSE read timed out" is thrown but the session retry never retries it | Open |
| [#22132](https://github.com/anomalyco/opencode/issues/22132) | Our exact hang: local Ollama hangs while `/v1/chat/completions` works directly | Open, no root cause, no workaround |
| [#18428](https://github.com/anomalyco/opencode/issues/18428) | Ollama takes 60-90s via OpenCode vs 3s direct; ~75s of OpenCode-side overhead suspected in streaming logic | **Closed as not planned** |

The 30s first-byte figure in #29420 matches the observed abort exactly, and the
broken `AbortSignal.timeout()` explains why provider `timeout` values have no
effect: the binary calls `AbortSignal.timeout(V.timeout)`, which is the
primitive that issue says does not work under Bun.

An [independent write-up](https://zenn.dev/masafumi_heijo/articles/opencode-ollama-timeout-tui-hang)
reached the same conclusion by a different route, including the distinction
that matters here: timeouts took effect under `opencode run` but not in the
interactive TUI. Their phrasing is worth keeping, since it describes most of
today: *"added and effective are two different problems."* They abandoned local
Ollama and standardised on Claude.

**The only workaround anyone confirms** is a third-party plugin,
[Mte90/opencode-auto-resume](https://github.com/Mte90/opencode-auto-resume),
which auto-resumes on timeout or error and exposes `chunkTimeoutMs`
(default 45000). Worth evaluating, but it resumes after a failure rather than
preventing one; pre-warming avoids the failure altogether and needs no plugin.

**What appears to be new signal:** no issue documents the TUI-versus-headless
asymmetry. Everything upstream reports the hang without noticing that
`opencode run` survives the same request. Measured here at 33.2s and 32.7s
headless against 30.4s and 30.5s cancelled in a pane. That is worth filing
upstream, since it localises the bug to the interactive path and is cheap for a
maintainer to reproduce.

## Dispatch headlessly (`opencode run`) instead of typing into the TUI

The 30 second ceiling on local-model requests is **not** an OpenCode-wide
limit. It belongs to the interactive session path only.

Measured both ways against a deliberately cold 20 GB model, which needs about
33s:

| Path | Ollama request | Outcome |
|---|---:|---|
| Mosaic pane (interactive TUI) | 30.4s, 30.5s | **cancelled both times** |
| `opencode run` (headless) | 33.2s, 32.7s | **completed both times** |

Two headless runs comfortably exceeded the limit that kills every pane request.
`BUN_CONFIG_HTTP_IDLE_TIMEOUT` made no difference and is not the cause; it was
tested and refuted rather than assumed.

Provider options cannot raise the pane ceiling either, and the compiled binary
shows why: the fetch wrapper collects abort signals and calls
`AbortSignal.any()`, which fires on the earliest. A signal is already attached
upstream (`t.signal`) before `timeout` / `headerTimeout` / `chunkTimeout` are
appended, so a longer value can never win.

So Mosaic has an architectural option worth weighing. It currently drives agents
by typing into an interactive CLI, which is what makes dispatch fragile in two
separate ways already documented here: the 1024-byte head truncation and this
30s ceiling. Running non-interactive work through `opencode run -m
provider/model "..."` would sidestep both, and would make large local models
usable, since laguna answers in 1.1s warm and only ever fails on a cold start it
is not allowed to finish.

The tradeoff is real and should not be waved away: the interactive pane is the
product. A user watching an agent work in a terminal is the point of Mosaic, and
a headless dispatch is invisible. A hybrid, where interactive panes stay as they
are and dispatched tasks run headlessly against the same session, is the shape
worth exploring, not a wholesale change.

## Local models: pick one that survives a cold start

Solved 2026-08-11, by measurement rather than tuning.

OpenCode enforces a hard **30 second** budget on a request to a local model.
It is not configurable: `timeout`, `headerTimeout` and `chunkTimeout` exist in
OpenCode's schema under `provider.<name>.options`, but setting them changed
nothing on the `/v1/chat/completions` path. Two requests either side of the
config change took 30.4172622s and 30.5398321s, so the option is simply not
honored there.

Measured against that budget, with OpenCode's ~13.2k-token prompt:

| Model | Size | Warm | Cold |
|---|---:|---:|---:|
| `lfm2.5:8b` | 5.6 GB | 9.6s | **13.9s** |
| `laguna-xs-2.1` | 20 GB | 11.2s | **32.7s** |

So a 20 GB model is 2.7 seconds too slow on a cold start, every time, and a
5.6 GB one has 16 seconds of headroom. Nothing here is flaky: a large local
model works until it idles out of memory, then fails deterministically on the
next request. That is exactly the "sometimes just stops working" symptom.

**Use small local models for OpenCode panes.** Large ones are viable only if
they never go cold, which is a guarantee nothing currently makes.

Supporting fixes already applied, both server-side because the client cannot be
configured:

- `OLLAMA_CONTEXT_LENGTH=32768`. The model's own context length is 262144;
  Ollama sized the KV cache from it, predicted 27.1 GiB, and evicted the model
  mid-session. Real usage was 1.2k-14k tokens. Note that `limit.context` in
  `opencode.json` does **not** control this: it caps what OpenCode will build
  into a prompt, not what Ollama allocates. Those are different things and
  conflating them wasted an afternoon.
- `OLLAMA_KEEP_ALIVE=30m`, up from the 5 minute default, so a thinking agent
  does not idle its model out and then pay a cold start it cannot afford.

Both are set in the user environment, but Windows only propagates that to
processes started after a fresh login, so Ollama must be launched from a shell
that already has them until you log out and back in.

Remaining lever if a bigger local model is ever wanted: shrink the 13.2k-token
prompt. Unverified whether disabling tools removes their definitions from the
request or only blocks execution (see sst/opencode#1320); it needs measuring
with a logging proxy, not assuming.

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

That pane was running **Laguna XS 2.1 locally through Ollama**, not a hosted
model, and the operator reports local models stopping like this is recurring
rather than a one-off. So this is not an exotic edge case to design around
loosely: on this machine it is the expected failure mode of an entire class of
session, and the pane most likely to die silently is the one whose work is
cheapest to hand out.

Two consequences worth separating:

- **Detection** is the item below: a dead pane's task must reach a terminal
  state.
- **Routing** belongs with model-aware dispatch: local panes are the wrong
  target for long, unattended, or on-the-critical-path work, however cheap they
  are. Cost is not the only axis; delivery probability is one too.

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

## Dispatch loses exactly the first 1024 bytes of a long prompt

**Measured, not guessed.** Three Codex dispatches arrived beginning mid-word.
Locating the survival point in the original text and adding the wrapper that
`dispatch_prompt` prepends gives the same answer twice:

| Dispatch | task chars lost | + prefix | total lost |
|---|---:|---:|---:|
| OpenRouter guardrails | 942 | 82 | **1024** |
| OpenCode timeouts | 942 | 82 | **1024** |

Two different prompts of different lengths, both losing exactly 1024 bytes from
the head. That is a 1 KiB buffer, not a race and not contention, and it kills
the earlier hypothesis that a concurrent fan-out was to blame.

Short dispatches are unaffected, which is the constraint that makes this
interesting: if the first 1024 bytes were always dropped, a 200-byte dispatch
would arrive empty, and those work fine. So the loss appears only once the
payload exceeds one chunk. A chunked writer whose first chunk is overwritten,
or lost to a redraw before the target's input is ready, fits the evidence.
`submit_to` frames Codex payloads with `PASTE_START`/`PASTE_END`, so the codex
path is the one to inspect first.

**A correction worth keeping.** An earlier entry here claimed a length-matched
test arrived intact and concluded this was not length-related. That test only
asked the agent to echo the *final* words and an end token, so it could not have
detected head truncation and almost certainly lost its own first 1024 bytes into
the filler. Testing only the end of a message cannot prove the beginning
arrived.

Silent corruption is worse than a failed dispatch: the agent does competent work
on the wrong brief, and in two of three cases said so only because it happened
to notice. Dispatch should verify what landed, or fail loudly.

Distinct from the submit race in `IMPROVEMENT-AUDIT.md` #1, which drops the
Enter rather than the text.

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

**Partly done, 2026-08-11.** `~/.config/opencode/opencode.json` now pins both
`model` and `small_model` to `openrouter/openrouter/free`, the Free Models
Router, with `max_price` zeros and `allow_fallbacks: false`.

The router is a better guard than pinning one `:free` model, for a reason worth
keeping: it cannot drift to a paid model, and it survives a model leaving the
free pool. That is not hypothetical, `inclusionai/ling-3.0-flash:free` had
already disappeared between the MODEL-GUIDE snapshot and the live catalogue.
Its tradeoff is no stable model identity between requests.

Care is needed with the sibling routers. `openrouter/free` prices prompt and
completion at 0, but `openrouter/auto`, `openrouter/fusion`, and
`openrouter/pareto-code` all report `-1`, meaning variable and billable. Pinning
the wrong router looks equally tidy and spends money.

Still outstanding, and still the only real guarantee: the account-side state
(zero balance, auto top-up off, payment method removed, no BYOK keys). Config
is declared intent; the balance is the enforcement.
