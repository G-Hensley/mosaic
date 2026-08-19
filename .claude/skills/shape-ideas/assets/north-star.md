# <Idea name>

Durable design context. This file holds what stays true across increments. Anything
specific to one shippable slice belongs in that increment's spec instead.

Drop any section that does not apply rather than filling it with "N/A".

## Goal

One sentence. What is true after this exists that is not true now, stated in the
language of the person it serves.

## Who it serves

The concrete user and what they are doing when they hit this problem. Name their
current workaround, since that is the thing your design has to beat.

## Success evidence

The observation, some months out, that would say this worked. A metric with a
direction, a behavior that becomes common, a cost that disappears.

## Constraints

The things that cannot change: stack, team, deadline, budget, compliance, runtime
environment, systems this has to live alongside.

## Non-goals

What this deliberately does not do. Include the things a reasonable person would
assume are in scope and are not.

## Approach

The chosen direction and the criteria it was chosen against. Cover architecture,
main components and their boundaries, data flow, and how failures surface.

Keep this at the level someone needs to understand the shape. Implementation detail
belongs in the increment specs.

## Rejected alternatives

| Option | Why it lost | What would change this |
|---|---|---|
| <option> | <the specific reason> | <the condition that would make it right> |

This table is what stops the same debate from reopening later.

## Assumptions

Carried from the interview. Mark the load-bearing ones.

| Assumption | Load-bearing | If wrong |
|---|---|---|
| <assumption> | Yes / No | <what breaks or changes> |

## Risks

| Risk | Resolution |
|---|---|
| <risk> | Accepted because <reason> / Mitigated by <design choice> / Experiment in <increment> |

## Increment ladder

The sequence, with one line each. The next increment is specified in full in its
own file; later ones stay as intent until the one before them ships.

| Increment | Outcome | Status |
|---|---|---|
| v0 | <what a user can do> | Specified |
| v1 | <what a user can do> | Sketch |
| v2 | <what a user can do> | Sketch |

## Open questions

What is still unknown, who can answer it, and which increment needs the answer.
