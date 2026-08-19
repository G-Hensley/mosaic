# Definition of Good

"Done" says the work stopped. "Good" says it was worth shipping. Every increment
needs the second one, written before the work starts, because a bar set afterward
is just a description of whatever happened.

## Per pass: the gates

Each interview pass has an exit condition. Meeting it is that step's definition of
good. Do not declare a pass complete; state how it is complete.

| Pass | Good means | Common false pass |
|---|---|---|
| Frame | The goal fits in one sentence the person agrees with, and the exclusions are explicit | Restating the person's request back to them and calling it a goal |
| Shape | One direction is chosen, the reason is recorded, and the rejected options are recorded with why | Presenting options and letting the discussion drift without a decision |
| Edges | Every identified risk is accepted with a reason, mitigated in the design, or scheduled as an experiment | Listing risks without resolving any of them |
| Slice | The first increment is small enough to build and its evidence is agreed | A ladder where the first rung is "build the foundation" |

## Per increment: the five parts

### 1. Outcome

What someone can do after this ships that they could not do before. One sentence,
in the language of the person using it, not the system implementing it.

If the outcome can only be stated in implementation terms, the increment is a task,
not a slice.

### 2. Evidence

The specific observation that proves the outcome is real. It must be reproducible
or independently checkable, rather than resting on the author's assertion. An
automated test the author wrote is fine evidence, because anyone can run it; "I
checked and it works" is not, because nobody else can.

Strong evidence is a named test, a demo path someone can walk, a metric with a
threshold, or a manual check with clear steps. "It works" is not evidence, and
neither is "tests pass" without saying which behavior they cover.

### 3. Quality bar

Only the dimensions that genuinely apply here, each with a threshold. A bar that
lists every dimension is a checklist nobody reads. Two or three real ones beat
seven generic ones.

Pick from what actually matters for this slice:

| Dimension | A threshold looks like |
|---|---|
| Correctness | The named edge cases behave as specified, including the empty and concurrent cases |
| Tests | The share-permission path has tests covering allowed, denied, and revoked |
| Performance | Report opens in under 2s at the 95th percentile with 50 rows |
| Security | A user without data access cannot open the report through any share path |
| Accessibility | Share dialog is keyboard operable and labeled for screen readers |
| Operability | A failed share attempt logs the reason and appears in the existing error dashboard |
| Documentation | The share behavior and its limits are written where support can find them |
| Data integrity | Revocation takes effect immediately and cannot be bypassed by a cached link |

### 4. Out of scope

What this increment deliberately does not do, so it can ship. This is what protects
the slice from growing, and it is the part most often left out.

Name the things a reasonable person would expect and will not get, so nobody has to
guess whether they were forgotten or excluded.

### 5. Reversibility

How to back this out, and whether it opens a one-way door. Name the migration cost
if there is one. An increment with an unstated rollback path is a bet nobody agreed
to make.

## Strong and weak, side by side

Same increment, written twice.

**Weak:**

> **v0: Report sharing**
> Users can share reports. Done when the feature works and tests pass. Should be
> secure, performant, and well documented.

Nothing here is checkable. "The feature works" cannot be disagreed with before it
is built, "secure" has no threshold, and there is no scope boundary, so the slice
will grow until someone gets tired.

**Strong:**

> **v0: View-only share link**
>
> **Outcome:** A report owner generates a link, and a teammate who already has
> access to the underlying data opens the report read-only without asking the
> owner for anything.
>
> **Evidence:** Demo path: owner creates a link, a second account with data access
> opens it, a third account without data access is refused. Tests cover all three.
>
> **Quality bar:**
> - Security: a user without underlying data access is refused through the link
>   path, and this is the case the tests assert first.
> - Correctness: revoking the link takes effect on the next request, not on a
>   cache expiry.
> - Performance: shared view loads within the same budget as the owner's view.
>
> **Out of scope:** named-recipient sharing, revocation UI, view counts, shared
> folders, email notification, mobile layout.
>
> **Reversibility:** link table is additive and can be dropped; no change to
> existing report or permission schemas. No one-way door.

The difference is not length. It is that every line in the strong version can be
checked by someone who was not in the conversation.

## The self-check

Before accepting a definition of good, ask:

1. Could someone who was not in this conversation tell whether it was met?
2. Would this definition pass for a different increment? If yes, it is too generic
   to be useful.
3. Can someone else reproduce or check the evidence without taking your word?
4. Does the quality bar name thresholds, or only adjectives?
5. Is there something a reasonable person would expect that is neither in scope nor
   named as out of scope?
