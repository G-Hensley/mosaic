# Description and Naming Discipline

The `description` is the primary discovery signal. It must tell a host both what capability is available and when that capability is relevant, without duplicating the workflow.

## Description formula

Use this shape:

```text
<Capability>. Use when <concrete triggers, artifacts, symptoms, or user requests>.
```

Examples:

```yaml
# Too vague
description: For async testing.

# Too much workflow
description: Run tests, replace sleeps with polling, rerun three times, and report flaky cases.

# Capability plus trigger
description: Diagnose and repair timing-dependent tests. Use when tests are flaky, hang, race, or rely on arbitrary sleeps and timeouts.

# User-language coverage
description: Create command-line tools and automation scripts. Use when building a CLI, batch job, scheduled task, data pipeline, or repeatable script.
```

## Rules

- Name the capability plainly.
- Include concrete triggering conditions and natural user language.
- Front-load distinctive nouns, file types, tools, or error symptoms.
- Keep process steps and implementation details in the body.
- Avoid promises broader than the actual workflow and resources.
- Keep the description concise; the open format permits up to 1024 characters, but most descriptions should be much shorter.
- Write in third person or neutral imperative language.

For technology-specific skills, name the technology. For general skills, describe the problem rather than an accidental implementation detail.

## Naming

Use a short kebab-case name that matches the skill folder exactly:

```text
GOOD                         WEAK
create-subagent              subagent-stuff
review-migrations            database-help
trace-root-cause             debugging-techniques
build-cli                    automation
```

Prefer verb-led names because they make responsibility clear. Use nouns only when the skill is genuinely a reference domain rather than a workflow.

Hard constraints:

- lowercase letters, digits, and hyphens only;
- no leading or trailing hyphen;
- no consecutive hyphens;
- maximum 64 characters;
- exact match between `name` and parent directory.

## Self-check

Before shipping, verify:

1. Could an agent identify what the skill provides from the description alone?
2. Could it identify when to load the skill?
3. Are the likely user phrases and artifact names present?
4. Are workflow steps left in the body?
5. Does the name distinguish this skill from its nearest neighbor?
