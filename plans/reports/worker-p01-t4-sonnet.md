# Worker report: phase 1, task T4 (docs/i18n.md)

Status: DONE_WITH_CONCERNS

## What changed

Created `docs/i18n.md` (305 lines). It fixes:

- Web library: `next-intl` 4.14.9 (registry-checked 2026-11-01), locale in path, static export; Paraglide JS as the fallback behind a bundle-size proof-of-concept gate (5% of first-load payload).
- Catalogue: ICU MessageFormat nested JSON; source `locales/en.json`, one `locales/<tag>.json` per locale, metadata in `locales/locales.toml` (TOML so the Weblate mask `locales/*.json` cannot match it). Namespaces `common.*`, `web.*`, `mac.*`, `legal.*` and a portable ICU subset for shared strings.
- macOS: `Localizable.xcstrings` generated from the JSON (mapping table for arguments, plurals, select, review state), committed, CI-verified, never hand-edited.
- TMS: hosted Weblate (`json-nested`, monolingual, `icu-message-format` flag, PR against `dev`, commit template that passes `committed.toml`).
- Machine-translation bootstrap rules, per-locale review status and completeness gate (reviewed 100%, community 80%), RTL (`ar`, `fa`), pseudo-locales `en-XA` and `ar-XB`, string-freeze rule, CI checks.
- Locale list: 42 locales.

## Locale list source

The Stirling report (`researcher-261031-2046-stirling-pdf.md`, row "Localisation: 40+ languages") only says "40+", with no list. I read the actual list from the upstream repo with `gh api`: `frontend/src/editor/core/i18n/languages.ts` (`supportedLanguages`, 42 entries; `rtlLanguages` = `ar-AR`, `fa-IR`) at commit `e16a5f92c715`, and `frontend/public/locales/`. Stirling already includes `vi-VN`, so adding `vi` leaves the count at 42. Tags are normalised to BCP 47 (`no-NB` to `nb`, `zh-BO` to `bo`, `zh-CN`/`zh-TW` to `zh-Hans`/`zh-Hant`, `sr-Latn-RS` to `sr-Latn`, region dropped where only one variant exists). Only language names and tags were taken, no translation text.

## Commands and results

- `gh api` on Stirling-PDF (locales dir, `languages.ts`, latest commit on the locales path): list and RTL set confirmed.
- `npm view next-intl`: 4.14.9, peer range includes `next ^16`.
- Weblate docs/hosting page fetched with curl: `json-nested` format and `icu-message-format` flag exist; Libre plan is free for public projects.
- `just spell`: exit 0.
- `just fmt-check` not run (Rust-only, no Rust files touched).

## Decisions I made where the brief was silent

- Scope excludes CLI, MCP, API and engine-protocol text (stay English).
- `en-GB` is an overlay of `en`; `en` is the US-spelling source.
- Completeness thresholds (100% / 80%), the 7-day freeze and the 5% bundle gate are my proposals.
- `ar` and `fa` stay hidden from the picker until a native reader sets `rtl_checked = true`, following the research advice not to auto-publish RTL copy.
- `legal.*` is never machine-translated.
- The Weblate GPL licence concern from the report is dropped because the service is hosted.

## Concerns / open questions

1. Free Libre plan eligibility is unverified (needs the owner's hosted project).
2. Machine-translation engine not chosen.
3. next-intl bundle size unmeasured; the doc gates it.
4. No named reviewers for `en`/`vi`, none for `ar`/`fa`.
5. The ARCHITECTURE.md Decision Log row (owned by T1) says "Library and TMS chosen in this phase"; T1 should link to `docs/i18n.md`.
6. The workspace pins `next` 16.4.2 while the research cites 16.4.3; irrelevant to the decision.
