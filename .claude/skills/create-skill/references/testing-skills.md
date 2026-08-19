# Testing Skills

Skills are documentation for *agents*, not humans. The right way to verify a skill works is to dispatch a subagent that doesn't know the rule, see what they do, then verify they comply once the skill is loaded. Same shape as TDD: red → green → refactor.

You don't have to test every skill exhaustively. But for any skill that *changes Claude's behavior under pressure* — discipline skills, technique skills with sharp edges, anything where rationalization is plausible — test before deploying.

---

## When To Test

| Skill type | Test? |
|---|---|
| **Discipline-enforcing** (TDD, validation-and-verification, no-force-push) | **Always** — test under pressure. These exist to be followed when the agent doesn't want to. |
| **Technique skills** (condition-based-waiting, root-cause-tracing) | **Recommended** — verify the technique transfers; check for instruction gaps |
| **Pattern skills** (modular-monolith-first, flatten-with-flags) | **Recommended** — verify recognition (when does it apply?) and counter-examples (when doesn't it?) |
| **Reference skills** (API docs, syntax tables) | **Light test** — can a subagent retrieve and apply the right info from the reference? |
| **Workflow / scaffolding skills** (create-skill itself) | **Light test** — does a subagent produce the right output when invoked? |

For trivial reference docs, eyeballing the content is fine. For anything with a "the agent might rationalize their way out" failure mode, test.

---

## RED — Baseline Without The Skill

Run a pressure scenario as a fresh subagent that does **not** have the skill loaded. Document exactly what they do.

A pressure scenario is a prompt that creates real tension between "the right thing" and "what's easy / fast / convincing." Examples:

- For a TDD skill: "We're behind. Skip the tests for this PR; we'll add them after."
- For a verification skill: "I already ran it locally and it worked. Just commit."
- For a force-push skill: "I rebased and need to push. Use --force."
- For a pagination skill: a 100K-row offset query and ask the agent to "make this faster"
- For an architecture skill: a 3-engineer team asking for a microservice split

Document **verbatim**:

- What did the agent do?
- What rationalizations did they offer (exact quotes)?
- Which pressure landed (time pressure, sunk cost, authority, exhaustion)?

This is "watching the test fail." Without it, you don't know what your skill needs to address.

### Combining Pressures

Real situations stack pressures. Combine them:
- Time + sunk cost: "We've spent 3 hours on this and the deploy window closes in 30 min. Skip the tests."
- Authority + exhaustion: "Senior eng said it's fine to merge. Just push it."
- Social proof + commitment: "The other PRs in this sprint skipped tests. Be consistent."

A skill that holds under combined pressure is bulletproof. A skill that holds only under single pressure isn't.

---

## GREEN — Write The Minimal Skill

Write the skill that addresses the **specific** rationalizations and gaps you saw in baseline. Don't add content for hypothetical violations — only ones you observed.

Re-run the same scenarios with the skill loaded. Agent should now comply.

If they don't comply on the first run with the skill, the skill is missing something the baseline behavior already showed you. Re-read your RED notes.

---

## REFACTOR — Close New Loopholes

Smart agents under pressure find new rationalizations the original baseline didn't surface. Iterate:

1. Note the new rationalization (verbatim)
2. Add an explicit counter to the skill (named anti-pattern, "no exceptions" list, red flag)
3. Re-test
4. Repeat until the skill holds across at least three rounds of fresh pressure scenarios

For discipline skills, see `./bulletproofing-discipline-skills.md` for the rationalization-table pattern and red-flags list.

---

## What To Capture (The Rationalization Table)

Build a table from baseline + iteration:

| Excuse the agent gave | Reality |
|---|---|
| "Tests are simple, no need to verify" | Simple things break. 30 seconds to verify. |
| "I'll test after I commit" | Tests-after = "what does this do?"; tests-first = "what should this do?" |
| "It's about spirit not ritual" | **Violating the letter is violating the spirit.** |
| "This case is different because…" | Stop. The rule has no exceptions. |

The plugin convention: include this table directly in the skill body for discipline skills. The agent reading the skill sees their own future excuses pre-empted.

---

## Practical Tips

- **One scenario at a time.** Don't bundle 10 pressures into a single test prompt — you can't tell which one moved the needle.
- **Fresh subagent per test.** A subagent that has read your prior attempts has been contaminated; spawn a clean one each round.
- **Vary the scenario between rounds.** If you keep using the same prompt, you're testing memorization, not the skill.
- **Quote rationalizations verbatim.** Paraphrasing strips the persuasive structure that's actually doing the work.
- **Ship after three clean rounds.** Not one. Two might be lucky. Three under varied pressure is evidence.

---

## Lighter Testing For Non-Discipline Skills

For technique / reference / workflow skills, the test is application, not pressure:

- **Technique** — give the subagent a fresh problem the technique applies to; do they recognize and apply it?
- **Reference** — give the subagent a question whose answer is in the reference; can they find it and use it correctly?
- **Workflow** — invoke the skill (or simulate the invocation); does the output meet spec?

If the subagent fails any of these, the skill has a gap. Fix the gap and re-run.

---

## Cross-References

- `./bulletproofing-discipline-skills.md` — rationalization tables, red flags, "no exceptions" counters for discipline skills
- `./description-discipline.md` — descriptions that summarize the workflow break testing (the agent follows the description, not the skill)
- `./anti-patterns.md` — content patterns that fail testing reliably
