# Contributing to Mosaic

Mosaic is a working prototype (see the README's "Known gaps" section), so
expect some rough edges. Contributions, bug reports, and design feedback are
welcome.

## Prerequisites

Mosaic is Windows-only; the terminal layer is ConPTY. You will need:

- [pnpm](https://pnpm.io)
- The [Rust toolchain](https://rustup.rs) (stable)
- The [Tauri CLI prerequisites for Windows](https://v2.tauri.app/start/prerequisites/), including the Visual Studio 2022 Build Tools. `dev.cmd` and `build.cmd` both call `vcvars64.bat` for you, so a plain `cargo build` from a regular shell will not link correctly; use the provided scripts.

## Setup

```powershell
pnpm install
.\dev.cmd        # sets up the MSVC environment, then `pnpm tauri dev`
```

## Building

```powershell
.\build.cmd
```

Artifacts land in `src-tauri/target/release/` (`mosaic.exe` and the NSIS
installer under `bundle/nsis/`).

## Testing

```powershell
cd src-tauri
cargo test
```

This covers the PTY submit-timing logic and the shared-brain prompt
formatting. There is no frontend test suite yet; a change to `src/` should at
minimum be exercised manually with `.\dev.cmd` and checked with:

```powershell
pnpm build   # runs `tsc` in strict mode, then the Vite build
```

## Branching and commits

Branch off `main` with a short, descriptive name in the style already used in
this repo, for example `fix/dispatch-submit-race` or `feat/task-board`.
Commit messages are sentence-style and imperative ("Wait for a quiet target
before submitting a dispatch"), not Conventional Commits prefixes. Keep
commits scoped to one change so the history stays readable.

## Review before you commit

If a change was made with the help of a Mosaic session, or any AI coding
agent, it needs a second pair of eyes from a different model before it lands,
not a rubber stamp from the model that wrote it. The workflow:

1. **Implement.** One session (the implementer) makes the change and gets it
   working: tests passing, `pnpm build` clean, no unrelated files touched.
2. **Route to a different-model reviewer.** Hand the diff to a session
   running a different model than the implementer, for example Claude Code
   implemented and Codex or opencode reviews. If Mosaic is not available, any
   second reviewer works, but the model actually has to differ, not just be a
   fresh prompt to the same one.
3. **Reviewer reports findings.** The reviewer reads the diff against the
   original request, not just the code in isolation, and returns concrete
   findings: correctness, scope creep, security-relevant paths, missing
   tests.
4. **Implementer fixes and asks for a recheck.** Do not self-certify a fix;
   send it back to the same reviewer for a second pass on just the delta.
5. **Reviewer approves.** Only once the reviewer has no more findings, or the
   remaining ones are explicitly accepted and noted in the PR, does the
   change move to commit.
6. **Commit and push.** Fill in the PR checklist below with what the review
   actually found.

This exists to catch what a single model misses about its own output, not to
slow down every one-line fix. Use judgment: a typo in a comment does not need
a cross-model review. A change to `mcp.rs`, `worktree.rs`, session identity,
dispatch, or anything else in `SECURITY.md`'s scope always does.

## Pull requests

**Never commit to `main`.** Every change lands on its own branch and through a
pull request, whatever its size: a bug fix, a feature, a doc correction. Branch
before editing, named for the work (`fix/dispatch-drops-leading-chunks`), and
keep one branch to one coherent change. When a branch has grown past what a
reviewer can hold in their head, split it rather than letting it accumulate.

This project **explicitly requires a pull request**, so opening one is part of
finishing the work rather than something to ask about first.

Propose the work when it is done, not when someone notices. On 2026-08-13 this
repository had 16 commits sitting on one branch with no PR, including the
dispatch truncation fix, on a branch whose name had stopped describing its
contents. Nothing was lost, but the work was invisible to everyone except the
session that wrote it, and by then it was too large to review in one sitting.

Open a PR against `main` using the PR template; it captures the checklist
below. Describe what changed and why, and call out anything that touches the
areas covered in `SECURITY.md` (the MCP server, worktree isolation, or
process spawning) so it gets a closer look. Please run `cargo test` and
`pnpm build` locally before requesting review. CI repeats those checks and
runs dependency, secret, and static-analysis scanners on pull requests.

### PR checklist

- Implementer: model and session (for example, Claude Code, sess-2).
- Reviewer: model and session, different from the implementer.
- Review result: approved, or approved with noted exceptions listed.
- Tests: `cargo test` and `pnpm build` both run locally, with pass or fail
  noted.
- Security impact: none, or a short description if the change touches the
  MCP server, worktree isolation, process spawning, or IPC.
- Confirmation that this diff contains only the intended change, with no
  unrelated files or another session's untracked or uncommitted work pulled
  in.

## Code style

- Rust: no repo-wide `rustfmt.toml` or `clippy.toml` yet; match the style of
  the file you are editing and run `cargo fmt` before committing.
- TypeScript: `tsconfig.json` has `strict` on with `noUnusedLocals` and
  `noUnusedParameters`; `pnpm build` will fail on either. There is no
  ESLint or Prettier config in the repo yet, so again, match the surrounding
  file.

## Scope note

`ui-gallery/` holds standalone HTML design explorations, not part of the
build. It is a useful reference for planned UI (see the README's "Known gaps"
and `IMPROVEMENT-AUDIT.md`) but changes there do not affect the app.
