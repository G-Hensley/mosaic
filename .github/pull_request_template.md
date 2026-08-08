## What changed and why



## Cross-model review

See [CONTRIBUTING.md](../CONTRIBUTING.md#review-before-you-commit) for the
full process. Summary here:

- [ ] Implementer: model and session (for example, Claude Code, sess-2)
- [ ] Reviewer: model and session, different from the implementer
- [ ] Review result: approved / approved with noted exceptions (list below)

## Verification

- [ ] `cargo test` run locally: pass / fail
- [ ] `pnpm build` run locally: pass / fail

## Security impact

- [ ] None
- [ ] Touches the MCP server, worktree isolation, process spawning, or IPC (describe below)

## Scope

- [ ] This diff contains only the intended change; no unrelated files or
      another session's untracked or uncommitted work are included

<!-- Noted exceptions, security details, or anything else the reviewer should see: -->
