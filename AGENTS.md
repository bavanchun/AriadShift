# AGENTS.md

Rules for AI agents working in this repository. Design authority is [ARCHITECTURE.md](ARCHITECTURE.md); read the section you are touching before changing structure, stack or boundaries.

## Current state

The CLI converts Markdown (`.md`/`.markdown`) to DOCX with `ashift convert`. Run `just pandoc` before working on conversion tasks. Run `just ci` as the local quality gate.

## Architecture changes

- Do not add a crate, app, service or dependency that contradicts ARCHITECTURE.md. If a change is warranted, update ARCHITECTURE.md, including its "Decision Log" table, in the same change.
- Conversion logic belongs in `ariad-core` or `ariad-host` only. Surfaces (CLI, desktop, web, server) never carry their own conversion code.
- Third-party engines run out of process through the engine protocol. Never link them into the host.
- The user-facing CLI binary is `ashift`; Rust crate names retain the `ariad-*` prefix.

## Versions

- Use the latest LTS where a channel exists, otherwise the latest stable release. Never use alpha, beta or RC builds. A date-pinned Rust nightly toolchain is the sole exception, used only by the scheduled sanitizer fuzz job.
- Verify a version against its registry (npm, crates.io, PyPI, GitHub Releases, endoflife.date) before pinning it. Do not pin from memory.
- Pin exact versions through lockfiles and commit the lockfile with the manifest.
- Package managers: `pnpm` for JS, `uv` for Python, `cargo` for Rust. Do not use npm, yarn or pip.
- Root tasks go into `just` recipes. Do not add Turborepo or Nx.

## Licensing

- Distributed artifacts must not contain AGPL or non-OSI code. This rules out PyMuPDF, `pdf2docx` and MinerU.
- GPL tools such as Pandoc are invoked as separate processes, never linked.
- Fixtures exclude CC-BY-SA documents, GPL test suites, and research-only or non-commercial datasets. Record each fixture's source and license in `fixtures/manifest.toml`.
- Model weights follow the same licensing rules as code; exclude OpenRAIL-M, custom or missing licenses.
- Never commit a user's or private document.

## Brand assets

- `brand/svg/`, `brand/png/`, `brand/icon-composer/` and `brand/preview.png` are generated. The build deletes and rewrites them, so never hand-edit them.
- Change `brand/src/*.mjs` or `brand/build.mjs`, then run `pnpm --dir brand install` (first time only) and `pnpm --dir brand build`.
- After a build, open `brand/preview.png` and check the 16px and 32px tiles before committing.
- Commit the sources and the regenerated outputs together.
- Keep SVGO's `prefixIds` plugin. Without it, IDs collide when several logos are inlined into one page.
- Design rules and the palette live in [docs/brand/design-direction.md](docs/brand/design-direction.md). Update that file when the design changes.

## Docs and language

- Write every repository file in English. Reply to the user in Vietnamese.
- Markdown goes under `docs/` or `plans/`. Root markdown is limited to `README.md`, `ARCHITECTURE.md`, `AGENTS.md` and `CLAUDE.md` (which only imports `AGENTS.md`; put rules in `AGENTS.md`).

## Git

- Follow [docs/git-workflow.md](docs/git-workflow.md) for the full Git workflow.
- Work lands on `dev`. `main` is the stable branch: it receives `dev` only through a promotion, plus hotfixes, and release tags are cut on it.
- Commit only when the user asks; an authorized plan run counts as that request for commits within the plan's scope.
- Pushes and GitHub settings require the user's explicit go-ahead.
- Stage explicit paths. Never run `git add -A` or `git add .` at the repository root.
- Use conventional commits (`feat(brand): …`, `docs: …`) with no AI or tool references.
- Never commit `node_modules/`, `.env*`, credentials or private keys.
