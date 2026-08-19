# Interview Passes

The interview is the work. These are the question banks, the recap format, and the
assumption ledger that make multiple passes feel like a conversation instead of an
intake form.

Never read a bank aloud in order. Pick the two or three questions that would most
change the design, assume the rest, and show the assumptions.

## Pass 1: Frame

Establishes what is being built and why. The most common failure in this pass is
accepting the person's proposed solution as the problem statement.

Ask about:

- The outcome. What is true after this exists that is not true now?
- The person it serves. One concrete user, not a segment. What are they doing when
  they hit this?
- The current workaround. What do they do today instead? A problem with no
  workaround is often not yet a real problem.
- The trigger. Why now? A deadline, an incident, a customer, a cost.
- Explicit exclusions. What is deliberately not part of this?
- Success evidence. What observation, six weeks from now, would say this worked?

Premise check, asked once and directly: state the problem you believe is actually
being solved. If the requested thing would not solve it, say so before designing.

> "You have asked for a caching layer. What I am hearing is that the dashboard
> takes nine seconds to load. If the cost is one slow query rather than repeated
> computation, caching hides it instead of fixing it and you inherit invalidation
> bugs. Do you know which it is?"

**Exit condition:** you can state the goal in one sentence the person agrees with,
and you know what is out of scope.

## Pass 2: Shape

Establishes how it will be built and which decisions actually matter.

Ask about:

- Constraints that are real: existing stack, team size and skills, deadline,
  budget, compliance, runtime environment, things that cannot change.
- Integration points. What does this have to live alongside or talk to?
- Data. What is stored, who owns it, what happens to it when this changes?
- Volume and growth. Today's numbers and the number that would break the design.
- Failure tolerance. What happens if this is down for an hour? For a day?
- Prior art. Has this been solved by something you could adopt instead of build?

Then present two to four approaches against stated criteria. See section 4 of
`../SKILL.md`.

**Exit condition:** one direction is chosen and the reason is written down, along
with what was rejected and why.

## Pass 3: Edges

Where designs actually fail. Skip at Quick depth, never skip at Deep.

Ask about:

- Failure modes. What breaks first under load, under partial failure, under bad
  input? What does the user see when it does?
- State and consistency. What happens on retry, on concurrent writes, on a crash
  halfway through?
- Security and access. Who can see and do what? What is the damage if a boundary
  is wrong?
- Migration. What exists already that has to keep working? What is the rollback?
- Operations. How does someone know it is broken? Who gets paged, and with what
  information?
- The pre-mortem. It is six months from now and this was a mistake. What happened?
  This question surfaces more than any other in this bank.

Turn each risk into one of three things: accepted with a reason, mitigated in the
design, or converted into a validation experiment that runs before the increment
that depends on it.

**Exit condition:** every identified risk is accepted, mitigated, or scheduled as
an experiment.

## Pass 4: Slice

Turns the design into a build order. See `increment-slicing.md` for the heuristics.

Ask about:

- The first real user and the first real use. Who touches v0, and when?
- The riskiest assumption. What, if wrong, invalidates the design? That goes first.
- The demo. What would you show someone to prove v0 works?
- Deferral. What feels essential but could ship in v1 without anyone suffering?
- One-way doors. What decisions here are expensive to reverse?

**Exit condition:** the first increment is small enough to build, and its
definition of good is agreed.

## The recap format

Recap at the end of every pass, in three to six lines, before starting the next.
This is what makes the passes work: it gives the person a cheap place to correct a
wrong turn before it compounds.

```markdown
**Frame (confirmed):** Cut dashboard load from 9s to under 2s for analysts
running the daily revenue view.
**In scope:** the revenue view only. **Out:** the other 14 dashboards, mobile.
**Assuming:** Postgres stays the store; no new infrastructure; single tenant.
**Open:** whether the 9s is one slow query or repeated computation.

Next I want to look at approaches. Anything wrong above before I do?
```

## The assumption ledger

Carry a running list from pass 1 and show it at every recap. Each entry is a
default you have adopted so the person does not have to answer a question.

Rules:

- State it as a decision, not a question: "Assuming Postgres stays the store"
  rather than "Should we keep using Postgres?"
- Mark anything load-bearing. If the design collapses when the assumption is
  wrong, say so, because that is worth the person's attention.
- Promote a load-bearing assumption to an actual question when its answer would
  change the increment boundaries.
- Move each entry into the north-star spec when the design is approved. Untracked
  assumptions are how specs quietly become wrong.

```markdown
| Assumption | Load-bearing | If wrong |
|---|---|---|
| Postgres remains the primary store | Yes | Query-level fixes do not transfer |
| Under 500 analysts, no burst traffic | No | Add caching sooner |
| No mobile support needed for v0 | No | Layout work moves into v1 |
```

## Question anti-patterns

| Anti-pattern | Why it fails | Instead |
|---|---|---|
| The intake form: eight questions in one message | The person answers three and drifts | Batch three or four independent ones, or ask the single blocking one |
| Serial questions with no memory | Feels like an interrogation, and the person repeats themselves | Recap each pass and carry the ledger forward |
| Asking what you could assume | Spends the person's attention on defaults | Put it in the ledger and let them correct it |
| Asking what does not change the design | Produces detail nobody uses | Drop it |
| Asking for a decision with no options | Offloads your job onto them | Bring two to four options and a recommendation |
| Interviewing past the point of value | The design stops improving and the person disengages | Present a concrete draft and let them react |
