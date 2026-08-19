# Increment Slicing

How to cut a design into versions that ship one at a time. The test for every cut:
if this shipped alone and nothing followed it, would someone be better off?

## The two rules that decide most cuts

**Independently shippable.** The increment can go to a real user without waiting on
anything after it.

**Independently valuable.** Someone is better off once it lands. If v1 is useless
without v2, they are one increment and you have cut in the wrong place.

Everything below is a consequence of these two.

## Slice by outcome, not by layer

The most common bad cut follows the architecture:

```text
BAD
v0: database schema and migrations
v1: API endpoints
v2: UI
```

Nothing is usable until v2. Every integration problem stays hidden until the
expensive end, feedback arrives after all the decisions are locked, and there is no
point at which you can stop and still have something.

The same work, cut by outcome:

```text
GOOD
v0: one user shares one saved report with one teammate by link, view-only
v1: shares go to named teammates with revocable access, and the owner sees who viewed
v2: team-wide shared folders with role-based permissions
```

Each of these is a thing a person can do. Each can ship and stop.

## A worked ladder

The design: let users share a saved report with their team.

**North-star (the durable version):** any team member can find, open, and subscribe
to reports their colleagues have built, with permissions that match the underlying
data access, and without asking the report author for anything.

**Riskiest assumption:** that permissions on the report can be derived from
permissions on the underlying data, rather than managed separately. If that is
false, the entire model changes, so it goes first.

| Increment | Outcome | Why here |
|---|---|---|
| v0 | Owner generates a view-only link; anyone with the link and existing data access can open the report | Tests the risky permission assumption on the smallest possible surface, one real user, one real report |
| v1 | Owner shares to named teammates, can revoke, and sees view counts | Adds the access control the link version deliberately skipped, now that the model is proven |
| v2 | Team-wide shared folders with roles | Only worth building once sharing is used enough to need organizing |

Note what v0 is not. It is not "the sharing infrastructure." It is a thing one
person can do end to end, chosen because it puts the riskiest assumption in front
of a real user in the least work.

## Experiments are not increments

Sometimes the most useful first step is not shippable: a throwaway spike against an
unfamiliar API, a load test, five user interviews, a prototype nobody will keep.
These reduce uncertainty without delivering anything to a user, and forcing them to
look like a product increment corrupts both ideas.

Keep them separate:

- **Validation experiment.** Reduces a specific uncertainty. Often disposable. Its
  definition of good is what you will know afterward and what decision it unblocks,
  not what a user can do.
- **Product increment.** Independently shippable and independently valuable, with
  the five-part definition of good.

An experiment can come before v0. Name it as an experiment, say what question it
answers, say what you will do with either answer, and time-box it. If you cannot
say what a result would change, it is procrastination wearing a lab coat.

## Sequence by risk, not by ease

The natural pull is to build the easy, well-understood parts first because progress
feels good. That defers the decision that could invalidate everything.

Ask: which assumption, if wrong, means rebuilding? Put the increment that tests it
first, even when it is the awkward one. A v0 that proves the hard thing and looks
unimpressive beats a v0 that looks finished and proves nothing.

## Specify the next one, sketch the rest

Write the next increment in full detail. Keep every later increment to a paragraph
of intent.

Detail written for v2 today is mostly wrong by the time v1 ships, because v1 is
what teaches you what v2 should be. Worse, detailed specs get defended: people
treat rewriting them as waste and steer toward the plan instead of the goal. A
paragraph is cheap to throw away, which is exactly why it survives contact with
reality.

Reshape the remaining ladder after each increment ships. That reshaping is the
process working, not the plan failing.

## One-way doors

Flag anything expensive to reverse, and place it as late as the design allows:

- Database schema changes that require a migration to undo
- Public API shapes, URL structures, and webhook payloads other people build on
- Data retention and deletion behavior
- Anything that writes data you cannot recompute
- Pricing, billing, and anything a customer signs

When a one-way door has to come early, say so in the increment spec's reversibility
section and name the cost of getting it wrong. When it can wait, keep the early
increments behind a flag, a manual step, or a smaller surface until the shape is
proven.

## Sizing

Keep an increment small enough to complete within one feedback cycle for this team
and project. What that means in calendar time varies enormously: a solo maintainer,
a six-person team, and a safety-critical system with a release board do not share a
number, so do not import one.

The test is feedback, not duration. If real feedback only arrives after several
unrelated capabilities are finished, the slice is too big regardless of how long it
takes. If you cannot see how to cut it smaller, the design is still unclear: go
back to the Shape pass rather than committing to a large slice.

More than four or five increments in a ladder usually means the north-star is two
projects. Split it, shape the first, and leave the second as a named idea.

## Bad cuts, and what they usually mean

| Bad cut | What it signals | Fix |
|---|---|---|
| Layer by layer (schema, API, UI) | Slicing by implementation structure | Cut by what a user can do |
| "v0: the framework, v1: the features" | Building infrastructure ahead of a proven need | Make v0 one real feature end to end; let the framework emerge |
| Every increment is "phase 1 of the real thing" | No increment is independently valuable | Find the one thing that helps someone alone, and start there |
| v0 contains everything, v1 is polish | Fear of shipping something incomplete | Move anything not needed for the core outcome to v1 |
| Increments split by team or component | Organizational structure, not user value | Cut by outcome and let teams coordinate within a slice |
| The risky part is in v3 | Sequencing by comfort | Move it to v0, even partially, as an experiment |
