---
name: research
description: Answer technical questions from freshly retrieved primary sources instead of model memory, with claim-level citations and explicit released-versus-draft status. Use when a question involves current versions, releases, deprecations, CVEs or security advisories, standards such as OWASP, SLSA, NIST, or CIS, cloud service capabilities, library or API behavior, pricing, EOL dates, or any request phrased as "latest", "current", "still supported", "is X vulnerable", or "did Y change".
---

# Research

Produce answers a reader can verify, from sources retrieved during this session, with the status and age of every load-bearing fact made explicit.

## The Iron Law

**NO FRESHNESS-SENSITIVE CLAIM FROM MEMORY. EVERY SUCH CLAIM CITES A SOURCE OPENED IN THIS SESSION.**

**Violating the letter of this rule is violating the spirit of it.**

The model does not own freshness. Tools own freshness. Your job is to reason over
evidence that was just retrieved, not to recall and then look for agreement.

## Workflow

### 1. Triage for freshness

Ask: does the honest answer to this question change over time?

Assume yes when the question touches versions, releases, deprecations, EOL,
security advisories, standards, cloud services, APIs, library behavior, defaults,
pricing, limits, supported platforms, or any recommendation that depends on any
of those.

If yes, memory is inadmissible as evidence; it may only decide where to look.
If genuinely no (stable algorithms, mathematics, settled history, the user's own
code in front of you), answer normally. Say which you concluded if it is close.

### 2. Check what you can actually reach

Tool availability differs by host and by project, so never assume a specific
tool exists. Check what you have: documentation retrieval (Context7),
repository access (a GitHub tool or API), a research CLI (`researchctl`), web
fetch, web search.

**Web search discovers; it is never evidence.** Everything else proves.

Degrade, do not fail: with only web search you can still open primary sources.
Say which capabilities were unavailable if that limited the result. Route by
domain using [source-policy.md](references/source-policy.md).

### 3. Retrieve

**Search discovers. Sources prove.**

A search result tells you where to look. It is not a citation. Open the
underlying page, specification, release, commit, API response, or advisory, and
read the part you intend to rely on. Source hierarchy is in
[source-policy.md](references/source-policy.md).

**When the question is "why is this software misbehaving", search for an
existing diagnosis first.** Check the project's issue tracker and the web
*before* investigating, not after testing several hypotheses. Measuring your way
to a conclusion someone already published is competent and wasted. A closed
issue is still a finding: "closed as not planned" means known, unfixed upstream,
and to be designed around.

**Opening an official page proves it is authentic, not that it is current.** A
versioned page stays official long after it stops being the answer. For any
"latest", "current" or "still supported" question, also retrieve the project's
**currency pointer**: its version index, release list, or current-version
banner. The artifact alone is not enough.

**When answering "latest", actively look for newer non-normative work** such as
a draft, release candidate, or preview, and report whether one exists. "No draft
exists" is a finding; silently omitting one is the same defect in the other
direction. Bound the search so it can be audited: check the currency pointer,
the default branch, and releases or tags, then **state what you checked**.

Beware a platform's own labels. A releases page may mark a rolling preview as
"latest" while the project's text says the previous stable release is still
normative. The project's word wins over the platform's badge.

### 4. Record status and dates as fields

For each load-bearing conclusion, record the fields in
[evidence-schema.md](references/evidence-schema.md). One compact record per
conclusion, not per sentence; ceremony gets abandoned.

**The fields must be visible in the answer, not captured privately.** Provenance
nobody can see is not provenance. If showing a field feels tedious, that is a
signal the claim is not load-bearing.

**Artifact status** is normalised to `normative`, `prerelease`, `superseded`,
`deprecated`, or `eol`. Keep the project's **own** word alongside it when they
differ: "Approved", "stable" and "GA" each mean something a normalised label
loses.

**Two separate dates**: when the source was published or last modified, and when
you retrieved it. Collapsing them hides staleness. If the page is undated, get
the date from the release announcement rather than omitting it.

A newer document is not automatically the current answer. A working draft is
newer than the release it will eventually replace, and reporting it as "the
latest" is the single most common way this work goes wrong. When both exist,
report both, and name which one is normative.

### 5. Cross-check what matters

A claim needs a second independent source when acting on it would change
production configuration, security posture, dependency choices, or architecture.

Independent means a different origin, not the same vendor page quoted by a blog.

**Currency claims are never exempt.** "Consequential" is a judgement you make
about your own work, so it is the easiest rule here to define your way out of.
Any claim about what is current, latest, or supported always needs both the
artifact and the currency pointer from step 3, regardless of how minor the
question seems. That pair is the minimum, not a bonus.

Currency claims and other consequential claims need *different* kinds of second
source, and conflating them lets one vendor's two pages pass as corroboration.
See [source-policy.md](references/source-policy.md).

When sources conflict, **report the conflict**. Do not average, do not silently
prefer the more recent, do not pick the one that fits the narrative. The
disagreement is itself a finding and usually the most useful one.

### 6. Report

- **Cite at claim level**, so a reader can check one sentence without reading
  the whole bibliography.
- **Separate observation from recommendation.** "The documentation states X" and
  "therefore I recommend Y" must look different.
- **Preserve reproducibility**: the exact query, the tag or commit, the
  canonical URL, the retrieval date.
- **Set confidence from evidence**, never from how familiar the topic feels.
- **State what you could not verify.** An explicit gap is a result; silence
  about one is a defect.

## Rationalization Table

| Excuse | Reality |
|---|---|
| "I know this one, it hasn't changed" | You cannot know that from inside the model. Checking costs one call. |
| "The search snippet already says it" | A snippet is not a source. Snippets are stale, truncated, and sometimes invented. Open the page. |
| "This draft is newer, so it is the latest" | Newer is not normative. Report the release and the draft separately. |
| "The top result is the official docs" | Then opening it costs you nothing. Open it. |
| "One good source is enough here" | For a consequential claim it is not. Independence is the point. |
| "The sources disagree, I will go with the better one" | Report the conflict. Choosing silently destroys the finding. |
| "It is close enough for a recommendation" | Then label it inference, not fact, and say so. |
| "The user is in a hurry" | A fast wrong answer costs more than a slow right one. |
| "I will caveat it as uncertain instead" | A caveat is not a substitute for a citation. |
| "There is no tool for this here" | Then say what you could not verify, and answer only what you can support. |
| "I captured the fields internally" | Provenance nobody can see is not provenance. Show it or drop the claim. |
| "It is the official page, so it is current" | Official and current are different properties. Get the currency pointer. |
| "This question is not consequential enough to cross-check" | Currency claims are never exempt. Artifact plus pointer, always. |
| "The release is confirmed, the draft is not worth mentioning" | Whether a draft exists is part of the answer. Say either way. |
| "The page has no date, so I will leave the date out" | Get it from the release announcement or release record. |
| "I will search the issue tracker if my own investigation stalls" | Search first. Prior art steers an investigation; it does not rescue one. |
| "That issue is closed, so it does not apply" | Closed as not planned means known and unfixed. That is an answer. |

**All of these mean: retrieve the source.**

## Red Flags, stop and reconsider

- You are about to state a version number, date, or limit you did not just read.
- You are about to cite a URL you did not open.
- You opened one official page and treated it as proof of what is current.
- You are about to say "latest" without having looked for a newer draft.
- You are about to write "latest" without saying latest *what*, as of *when*.
- You are about to resolve a conflict between sources by not mentioning it.
- You are about to present an inference in the same voice as an observation.
- You are reaching for hedging language because you could not verify something.

**All of these mean: stop, retrieve, then write.**

## No Exceptions

The Iron Law applies:

- Not for facts that feel obvious.
- Not for widely known projects.
- Not when the answer matches what you expected.
- Not when the user seems to already know the answer.
- Not under time pressure.
- Not when only one source exists. In that case, say that only one exists.

## Worked Example

"What is the latest OWASP guidance on authentication?"

**Wrong:** name a Top 10 edition from memory and summarise it.

**Right:** OWASP is not one versioned artifact. Resolve which project governs
the question, retrieve *that* project's current release and its status, and
report its version rather than an "OWASP version" that does not exist. Report
any newer draft separately. SLSA has the same shape: an approved specification
and a working draft coexist, and only one is normative.

## Resources

- [source-policy.md](references/source-policy.md) — authoritative source per
  domain, and the routing order for security, cloud, standards, and library
  questions.
- [evidence-schema.md](references/evidence-schema.md) — the fields to capture
  per claim, and the shape of the evidence record.
