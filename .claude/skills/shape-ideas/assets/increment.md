# <vN>: <short name>

One shippable, independently valuable slice. If this shipped and nothing followed
it, someone is better off.

## Outcome

One sentence. What someone can do after this ships that they could not do before,
in their language rather than the system's.

## Scope

What this increment includes, at the level of user-visible behavior. Enough for
someone to build it without reopening the design conversation.

## Out of scope

What this deliberately does not do, so it can ship. Name the things a reasonable
person would expect and will not get here, so nobody has to guess whether they were
forgotten or excluded.

## Definition of good

**Evidence:** the specific observation that proves the outcome is real, observable
by someone other than the author. A demo path, named tests, or a metric with a
threshold.

**Quality bar:** only the dimensions that genuinely apply, each with a threshold.
Two or three real ones beat seven generic ones.

- <dimension>: <threshold>
- <dimension>: <threshold>

**Reversibility:** how to back this out, the migration cost if any, and whether it
opens a one-way door.

## Dependencies

What must exist before this starts, and what is deliberately stubbed, flagged, or
done manually for now.

## Notes

Design decisions specific to this slice, and anything learned while building it
that should reshape the increments after it.
