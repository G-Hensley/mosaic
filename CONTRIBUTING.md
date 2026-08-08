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

## Pull requests

Open a PR against `main`. Describe what changed and why, and call out
anything that touches the areas covered in `SECURITY.md` (the MCP server,
worktree isolation, or process spawning) so it gets a closer look. Please run
`cargo test` and `pnpm build` locally before requesting review; there is no CI
running these yet.

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
