---
name: shape-ideas
description: Shape a rough idea into a validated design and a sequence of independently valuable increments, each with an explicit definition of good. Use when someone wants to brainstorm, whiteboard, or think through a feature, product, system, process, or rework before building; wants help scoping an MVP or a v0, v1, v2 ladder; wants to decide what ships first; or asks what "good" or "done" should mean for a slice of work.
---

# Shape Ideas

Turn a rough idea into a design the person actually believes in, then cut it into
increments that ship one at a time, each with a stated definition of good. The
value is in the interview, not the paperwork: specs are a byproduct of having
thought clearly, so never let producing them become the point.

## Do not run this skill when

- The change is mechanical and reversible in a single step: a rename, a config
  value, a dependency bump, a copy edit, a one-line fix.
- The person already has a decided design and is asking for execution.
- The request is a question to answer, not work to shape.
- The person explicitly asks to just build it. Offer this once, accept the answer,
  and do not re-offer in the same session.

Say what you are skipping and why in one sentence, then do the work directly.
Running a design ceremony over a trivial change trains people to route around you.

## 1. Set the depth

Pick from consequence, ambiguity, and reversibility. State the depth you chose and
let the person move it.

| Depth | Fits | Interview | Output |
|---|---|---|---|
| Quick | One reversible slice, low blast radius, clear intent | One round of clarification | Options and a recommendation in conversation |
| Standard | A feature with real unknowns or several moving parts | Passes 1, 2, and 4 | Increment specs, north-star only if more than one increment |
| Deep | A new system or product, or several interacting subsystems | All four passes plus risks and validation experiments | North-star spec and increment specs |

Consequence is not depth. A guarded area (auth, secrets, personal or production
data, schemas and migrations, deployment) raises the approval bar and makes the
Edges pass mandatory, but a tightly specified change inside one can still be Quick.
Depth can also change mid-session. If pass 1 reveals the idea is three projects
wearing a trench coat, stop and decompose before going deeper.

## 2. Diverge before you converge

Generate before you judge. Offer several genuinely different directions, including
at least one that questions the framing, and hold off on evaluating any of them
until the set is on the table.

Two moves that pay for themselves early:

- **Challenge the premise once, explicitly.** State the problem you believe is
  actually being solved. If the request would not solve it, say so before designing
  anything. This is the highest-value move here, and only works before investment.
- **Check prior art** when the problem is likely already solved. An existing
  library, pattern, or product that covers 80 percent changes the design.

Do not converge on one approach until the person has seen the alternatives.

When a choice turns on layout, spatial arrangement, or competing visual directions,
offer to show it rather than describe it, using whatever the host provides. A
question about a UI topic is not automatically a visual question.

## 3. Interview in passes

Multiple short passes, each with a stated exit condition and a recap. Work them in
order, and go back when later evidence changes an earlier answer: slicing routinely
exposes a constraint that reshapes the frame. The exits are checkpoints, not
one-way gates. Recap in three to six lines before moving on, so the person can
correct a wrong turn cheaply.

1. **Frame.** The outcome, who it is for, what changes when it works, what is
   deliberately excluded. Exits when you can state the goal in one sentence the
   person agrees with.
2. **Shape.** Approaches, constraints, and the decisions that actually matter.
   Exits when one direction is chosen and the reason is written down.
3. **Edges.** Failure modes, data and state, scale, security, operations,
   migration. Exits when the risks are known and each is accepted, mitigated, or
   turned into an experiment. Skip at Quick depth.
4. **Slice.** Increments, sequencing, and the definition of good for each. Exits
   when the first increment is small enough to build and its evidence is agreed.

Question discipline, in priority order:

- Ask the question whose answer most changes the design. If an answer would not
  change the design, the boundaries, or the definition of good, do not ask it.
- **Assume, do not interrogate.** Anything you can reasonably infer becomes a
  stated assumption the person corrects, not a question they answer. Keep a running
  assumption ledger and show it at each recap. Correcting is far cheaper than
  answering.
- Batch independent questions, up to three or four at a time. Ask serially only
  when the answer genuinely determines the next question.
- Stop when no unknown would change the design. Past roughly six questions in a
  pass, present a best-effort design with assumptions flagged instead.

See [interview-passes.md](references/interview-passes.md) for the question banks,
recap format, and the assumption ledger shape.

## 4. Compare approaches against stated criteria

Name the criteria before the options, so the recommendation is checkable rather
than asserted. Use criteria that actually discriminate here, and always include
reversibility.

Give two to four options, each with what it costs, what it buys, and what it
forecloses. Lead with your recommendation and the reason. Record the rejected
alternatives and why they lost, which is what stops the debate reopening in three
weeks. Never recommend without stating what would change your mind.

## 5. Cut the work into increments

Every increment must be independently shippable and independently valuable. If v1
is useless without v2, they are one increment.

- Slice by user-visible outcome, never by layer. "Database, then API, then UI"
  ships nothing until the end and hides integration problems until they are costly.
- v0 is the smallest thing delivering the core outcome for one real user on the
  happy path, with the riskiest assumption tested.
- Sequence by risk, not by ease. A validation experiment that reduces uncertainty
  may come before v0 without pretending to be a product increment.
- Specify the next increment in detail; keep later ones to a paragraph of intent.
  Detail written for v2 is mostly wrong by the time v1 ships, and people defend it
  rather than rewrite it.
- Flag one-way doors: schemas, public interfaces, data migrations. Defer them when
  you can, call them out when you cannot.
- More than four or five increments means the north-star is probably two projects.

See [increment-slicing.md](references/increment-slicing.md) for worked ladders, the
experiment-versus-increment distinction, and the common bad cuts.

## 6. Define good for every step

**Per pass:** each pass has an exit condition above. That is its definition of
good. Do not move on by declaring it met; state how it is met.

**Per increment:** every increment spec carries a definition of good with five
parts. Outcome, evidence, quality bar, out of scope, and reversibility. Evidence
must be reproducible or independently checkable rather than resting on your word,
and the quality bar names only the dimensions that apply, each with a threshold.

A definition of good that would pass for any increment is not one. See
[definition-of-good.md](references/definition-of-good.md) for the rubric and a
strong and weak example side by side.

## 7. Write the specs, proportionally

Write files only after the person approves the design, and only what the work
warrants. Name the location when you ask for that approval, so writing to their
repository is part of what they agree to rather than a surprise. If they would
rather keep it in the conversation, do that.

- One increment: write the increment spec alone.
- More than one increment, or real architectural breadth: write the north-star
  spec plus one spec per increment.
- Quick depth: write nothing unless asked.

Default location is `docs/design/<slug>/`, with `north-star.md`, `v0-mvp.md`,
`v1.md`, and so on. Project convention wins; look for an existing specs or design
directory before creating a new one.

Use [north-star.md](assets/north-star.md) and [increment.md](assets/increment.md)
as the starting shapes. Drop sections that do not apply rather than filling them
with "N/A".

Before handing the specs over, reread them once for placeholders, contradictions
between sections, requirements that could be read two ways, and scope that has
crept past a single increment. Fix what you find inline.

## 8. Stop at the boundary

Designing something is not permission to build it. This holds from the moment the
skill engages, not only once specs exist.

While the person has not asked you to build:

- Do not implement any of it.
- Do not commit, stage, or branch.
- Do not invoke another workflow, planning skill, or agent.
- Offer the next step as a question: ask whether they want to start on v0.

When they do ask you to build, that is authorization. Take it, do not argue, do not
re-offer the design. Three things still hold:

- Write or update the increment spec first, so what was agreed survives the
  session, and say in one line that you are moving from shaping to building.
- Committing is separate from building. Commit when asked, or when project policy
  directs it for completed work. Never commit merely because shaping finished.
- In a guarded area, the project's guarded-change policy still applies. An agreed
  design is not the scoped approval that policy asks for, because the design is
  what you would be seeking approval of.

An instruction to build is authorization. Enthusiasm, a passing remark, and your
own sense that the shaping is finished are not.

## Resources

- [interview-passes.md](references/interview-passes.md): question banks, recap format, assumption ledger.
- [increment-slicing.md](references/increment-slicing.md): worked ladders, slicing heuristics, bad cuts, validation experiments.
- [definition-of-good.md](references/definition-of-good.md): the five-part rubric, quality-bar dimensions, strong versus weak examples.
- [north-star.md](assets/north-star.md): template for the durable spec.
- [increment.md](assets/increment.md): template for one shippable slice.
