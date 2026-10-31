# UI Surface Technology Research: Document/PDF App

Date: 2026-10-31. Scope: the 10 items in the brief (A1-A5 web, B6-B10 macOS). Research only; no repo files were changed.

## Summary

1. **Web stack.** Use next-intl 4.14.9 for i18n, EmbedPDF 2.15.1 (PDFium WASM) as the PDF viewer and editing base with pdfjs-dist 6.4.299 as the fallback, Tiptap 3.32.2 for the reflow editor, and Motion, TanStack Virtual, Pragmatic DnD and Zustand for UI. BlockNote is excluded: its core is MPL-2.0 and its XL packages are GPL-3.0 OR PROPRIETARY.
2. **Translations.** Weblate 2026.10.1 first (Git-native, GPL-3.0 server kept outside the shipped app), Tolgee v3.241.3 second (Apache-2.0 core), Crowdin last (proprietary SaaS).
3. **macOS toolchain.** Xcode 27.2 (27B52, released 2026-10-29, Swift 6.4). Use UniFFI 0.32.2 with its own `uniffi-bindgen-swift` binary and `xcodebuild -create-xcframework`. Do not use cargo-swift 0.11.1, which pins `uniffi_bindgen =0.31.1`.
4. **Native PDF.** PDFKit for on-screen viewing, pdfium-render 0.9.4 (MIT OR Apache-2.0) in the Rust core for export and render parity with web. Updates use Sparkle 2.10.1 with EdDSA keys; notarization uses `notarytool`.
5. **Deployment floor.** Use macOS 15 as the floor and gate Foundation Models features behind a macOS 26+ availability check. macOS 14 reached end of life on 2026-09-14 (endoflife.date). macOS 27 is current (27.2 released 2026-10-26). Market-share figures are unverified.
6. **Hard constraints.** Static export ignores `headers` in next.config, so COOP/COEP must be set by the hosting layer. Foundation Models changes model behaviour with every OS update, so prompts need retesting each release.

## Method and source weighting

- Registry and repo facts (highest weight): npm `registry.npmjs.org`, crates.io API, GitHub API (licence files, releases, push dates), nodejs.org release index, endoflife.date.
- Official docs (high weight): Apple developer release notes and Markdown doc endpoints (Xcode 27 and 27.2 notes, Foundation Models updates, notarization, XCFramework, String Catalog), nextjs.org static-export and support-policy pages (fetched 2026-10-31), next-intl.dev docs, UniFFI manual at tag v0.32.2, Sparkle release notes on GitHub.
- Secondary sources (flagged as such): third-party blogs, forum threads, pub.dev listings, pricing aggregators, market-share sites.
- Where sources conflicted, the authoritative source won. Examples: Apple's release list shows Xcode 27.2 final, not the RC some blogs cite. The Next.js policy page names no minor as LTS, so third-party claims that 16.4 is LTS are rejected.

## A. Web

### A1. i18n for Next.js 16 App Router (static export, 40+ locales, RTL, ICU plurals)

| Option | Version (npm publish) | Licence | Static export fit | Bundle per locale | Type safety | Main risk |
|---|---|---|---|---|---|---|
| next-intl | 4.14.9 (2026-10-02) | MIT | Documented path: `next/root-params` (Next 16.3+), `generateStaticParams` required, `setRequestLocale` legacy | One locale's messages per HTML page; no per-message tree-shaking | Strong, app-wide types | Proxy-based locale detection does not run in static export; locale comes from the path |
| Paraglide JS | 2.29.3 (2026-10-28) | MIT | Core works with `generateStaticParams` (unverified in repo); the v1 Next adapter lists static export as unsupported | Smallest; compile-time per-message imports | Strong, generated functions | No maintained Next static-export adapter found; you write the glue |
| Lingui | 6.11.0 (2026-10-29) | MIT | Compiled catalogs, no server needed; Next wiring is manual (unverified) | Small compiled catalogs | Good with macros | Needs Node >= 22.19; macro toolchain under Next 16 unverified |
| i18next / react-i18next | 26.5.12 / 17.1.9 (2026-10-30) | MIT | Works client-side; no App Router integration | Largest runtime | Weak unless typed by hand | Biggest ecosystem, weakest App Router fit |

**Ranking:** 1) next-intl. 2) Paraglide, to switch to if the POC shows next-intl bundles are too large. 3) Lingui. 4) i18next.

**Why next-intl.** Its documented static path matches Next 16.4.3. Its peer range already includes `next ^16.0.0`. Static export splits pages per locale, so each page loads one locale's messages. The per-locale size is unmeasured in this run; a POC must measure it before commit.

**Next.js static-export limits** (nextjs.org docs, lastUpdated 2026-08-09): middleware/proxy, rewrites, redirects, headers, the default `next/image` loader, Server Actions, Draft Mode, and dynamic routes without `generateStaticParams` are unsupported. Locale must live in the path and be prerendered. Images need a custom loader or `unoptimized`.

**RTL.** Set `lang` and `dir` per locale in the root layout and use Tailwind 4 logical utilities (`ms-`, `me-`, `start-`, `end-`). Standard practice; not separately verified.

**Next.js LTS status.** The official support page lists 16.x as Active LTS (released 2025-10-21) with no end date. It lists 15.x as Maintenance LTS, with security releases through 2027-01-31. Next.js requires Node >= 20.9.0.

### A2. Translation management (40+ languages, self-hostable, EN in git as source of truth)

| Platform | Version (date) | Licence / hosting | EN source in git | Cost | Main risk |
|---|---|---|---|---|---|
| Weblate | 2026.10.1 (2026-10-09) | GPL-3.0 (GitHub API); self-host | Git-native VCS workflow (well known; not re-verified in this run) | Free self-host | GPL server must stay out of the app artifact; ops burden |
| Tolgee platform | v3.241.3 (2026-10-29) | Apache-2.0 outside `ee/` (LICENSE file); `ee/` is separately licensed | Via CLI/API sync (unverified detail) | Free self-host; paid cloud (2026 listings conflict: $89 vs $96.12/mo) | Must not use `ee/` code in the product |
| Crowdin | Proprietary SaaS | Free OSS licence on request (pricing page, verified); self-host not offered on that path | GitHub integration (unverified) | Paid Pro from about $50/mo (single listing, unverified) | Vendor lock-in; fails the self-host preference |

**Ranking:** 1) Weblate. 2) Tolgee. 3) Crowdin.

**Why Weblate.** Its Git-native model fits "EN is the source of truth in git" most directly. The GPL-3.0 server is a separate service and never linked into the app. Whether that is acceptable under the AGENTS.md artifact rule is an owner decision (see unresolved questions).

**Machine-translation bootstrap.** Generate MT or LLM drafts, commit them in a "needs review" state, and require native review before publish. Do not auto-publish RTL or legal copy. Specific free MT engines in Weblate are unverified.

**Process.** EN message catalog in git; CI check for missing keys; translation PRs or sync through the platform; a per-locale completeness gate before release.

### A3. In-browser PDF viewing and editing

| Option | Version (date) | Licence | Features | Performance on 1,000 pages | Main risk |
|---|---|---|---|---|---|
| EmbedPDF (`@embedpdf/react-pdf-viewer`, `core`, `plugin-annotation`, `plugin-redaction`, `plugin-thumbnail`, `plugin-search`, `plugin-selection`, `plugin-form`) | 2.15.1 (2026-09-16) | Apache-2.0 per repo LICENSING.md and `packages/core/package.json`; npm metadata says MIT (conflict) | PDFium WASM in a worker; viewer, annotations, true redaction, thumbnails, search, selection, forms | No public 1,000-page benchmark. One third-party device measurement (pub.dev listing) reports faster visible content and lower memory than pdf.js (unverified) | Young 2.x SDK; single vendor (CloudPDF) |
| pdfjs-dist | 6.4.299 (GitHub release 2026-10-03) | Apache-2.0 (repo) | Mature viewer, text layer, annotation display; no built-in redaction | Widely used; no verified numbers | You build redaction and editing yourself |
| `@embedpdf/pdfium` (raw PDFium WASM) | 2.15.1 | MIT wrapper; upstream PDFium licence unverified (believed BSD-3) | Low-level only | Same engine as EmbedPDF | No reason to pick it over EmbedPDF |

**Ranking:** 1) EmbedPDF. 2) pdfjs-dist, as fallback if the EmbedPDF POC fails. 3) Raw PDFium WASM.

**Why EmbedPDF.** One SDK covers viewer, thumbnails, annotations, redaction and forms. PDFium gives Chrome-grade rendering. The worker-based design fits the existing COOP/COEP cross-origin isolation. The trade-off is lower maturity than pdf.js, which is accepted with a fallback.

**POC gate.** On a 1,000-page fixture (fixture source and licence recorded per project rules), measure time to first page, per-page render time, peak memory, text-layer extraction, and thumbnail generation for all pages.

### A4. Rich-text/reflow editor

| Option | Version (date) | Licence | Tables | Collaboration | Main risk |
|---|---|---|---|---|---|
| Tiptap | 3.32.2 (npm 2026-10-30) | MIT core; `@tiptap/extension-table` and `extension-collaboration` MIT (npm) | Yes (MIT) | Yjs (MIT); server `@hocuspocus/server` 4.8.0 (MIT) | Paid pieces: cloud, Comments, Tracked Changes, AI Toolkit (pricing page; per-extension licence split unverified) |
| Lexical | 0.53.0 (2026-10-28) | MIT | `@lexical/table` MIT | `@lexical/yjs` MIT | Pre-1.0 (0.x) API churn |
| Slate | 0.147.4 (npm) | MIT | Community-built | Manual | Maturity status unverified; more DIY work |
| BlockNote | 0.57.0 (2026-10-30) | Core MPL-2.0; `@blocknote/xl-ai` and `xl-multi-column` GPL-3.0 OR PROPRIETARY | Yes | Yes | Excluded (see Licence gate) |

**Ranking:** 1) Tiptap 3. 2) Lexical. 3) Slate. BlockNote excluded.

**Notes.** The Tiptap GitHub latest release is v3.32.0 (2026-10-13), while npm is at 3.32.2; pin the npm version and check its tag. A "Word-like" reflow editor needs a structural model extracted from the PDF text layer. No editor solves PDF pagination; that is a product decision for the owner.

### A5. UI polish stack

| Need | Pick | Version (date) | Licence | Main risk |
|---|---|---|---|---|
| Animation | motion (Motion for React) | 14.9.0 (2026-10-30) | MIT | Large API; keep to layout and presence animations |
| Virtual lists, thumbnails | @tanstack/react-virtual | 3.14.24 (2026-10-29) | MIT | Low |
| Page drag-and-drop | @atlaskit/pragmatic-drag-and-drop | 4.0.0 (2026-09-24) | Apache-2.0 (LICENSE file; repo metadata NOASSERTION) | Fewer ready-made React helpers than dnd-kit |
| DnD alternative | @dnd-kit/core | 6.3.1 (npm 2024-12-05) | MIT | No npm release for about 23 months, though the repo was pushed 2026-10-31 |
| Command palette | cmdk | 1.1.1 (npm 2025-03-14) | MIT | Last release about 19 months old; repo last pushed 2025-10-29 |
| Command palette alt | @ariakit/react | 0.4.42 (2026-10-14) | MIT | Smaller shadcn-ecosystem footprint |
| State | zustand | 5.0.16 (2026-10-22) | MIT | Low |
| State alt | jotai | 3.0.1 (2026-09-29) | MIT | Low |

**Rankings.** DnD: 1) Pragmatic DnD (active, Apache-2.0), 2) dnd-kit. Command palette: 1) cmdk, accepted with risk; it is small and swapping to Ariakit is cheap. 2) Ariakit. State: 1) Zustand, 2) Jotai.

**Skip TanStack Query.** This is a local-first app with no server state, so it adds weight without benefit (KISS). Add it only when a sync backend exists.

## B. macOS native

### B6. Toolchain and UI architecture

- **Xcode 27.2** (build 27B52) was released 2026-10-29 per Apple's releases page. It includes Swift 6.4 and the macOS 27.2 SDK. It requires a Mac running macOS Tahoe 26.6 or later (Xcode 27.2 release notes). Earlier: Xcode 27.1 (27A9275) on 2026-10-15; Xcode 27 (27A266a) on 2026-09-14.
- **Concurrency.** Use Swift 6 language mode for strict concurrency. Specific Swift 6.4 concurrency changes are unverified beyond the release notes.
- **SwiftUI vs AppKit.** 1) SwiftUI `App` with `DocumentGroup` for the document lifecycle, multi-window, `NavigationSplitView` and inspector. 2) AppKit bridging through `NSViewRepresentable` for the PDF canvas and any toolbar behaviour SwiftUI cannot express. 3) Full `NSDocument` only if `DocumentGroup` cannot express a required behaviour (trade-off: more AppKit code to maintain).
- **Trade-off.** Each AppKit bridge adds maintenance cost. Keep the number of bridges small.
- **Xcode 27 build note.** Targets with a macOS 27.0+ deployment target no longer build universal by default (`ARCHS_STANDARD` drops x86_64). Set `ARCHS` explicitly if you ship x86_64 (release notes). Not relevant at a macOS 15 floor, but it matters for the build config.

### B7. UniFFI (Rust core to Swift)

| Option | Version (date) | Licence | Status | Verdict |
|---|---|---|---|---|
| uniffi (runtime) + own `uniffi-bindgen-swift` binary | 0.32.2 (changelog 2026-09-22; crates.io 2026-09-23) | MPL-2.0 | Manual at v0.32.2 documents `--xcframework --modulemap`; partial Swift 6 support | 1 |
| cargo-swift | 0.11.1 (2026-05-20) | Apache-2.0 | Pins `uniffi_bindgen =0.31.1`, behind 0.32.x | 2, only at 0.31 or after it updates |
| swift-bridge | 0.1.59 (2026-01-06) | Unverified | Hand-written bridge style | 3 |

**Build path.** Build Rust for `aarch64-apple-darwin` and `x86_64-apple-darwin`. Generate headers, modulemap and Swift sources with the bindgen binary (`--xcframework --modulemap`). Package with `xcodebuild -create-xcframework` (Apple doc present). The UniFFI repo ships `examples/app/ios` (xcode.md) as a reference.

**Async.** UniFFI maps Rust async to Swift `async`. Generated async code does not conform to `Sendable` (manual; tracked in issue #2448). Cross async results through actor-isolated wrappers. Zero-copy byte transfer is not supported in async functions (0.32 changelog).

**Errors.** Rust error enums become Swift `Error` enums, and functions with an error type are marked `throws`. Rust panics become a private error: in a non-throwing function they become a fatal error that cannot be caught (manual). Design inference: catch panics at the FFI boundary so they never reach that path.

**Stability.** UniFFI is 0.x with breaking changes per minor (0.31 to 0.32 changed Kotlin and Python async constructors and byte-buffer signatures). Pin exact versions and update deliberately.

**Shipping examples.** Named production apps are unverified in this run.

### B8. macOS viewer: PDFKit vs embedded PDFium

| Option | Version (date) | Licence | Strengths | Weaknesses |
|---|---|---|---|---|
| PDFKit | System framework (macOS 10.4+) | Apple SDK | Native scrolling, selection, find, thumbnails; zero binary weight; Apple-maintained | Tahoe bugs reported on Apple forums (anecdotal; no Apple acknowledgement found); render differs from web PDFium; macOS 27 status unverified |
| PDFium via pdfium-render | 0.9.4 (crates.io 2026-09-06); MIT OR Apache-2.0 | Crate dual licence; PDFium binary from bblanchon/pdfium-binaries (MIT build scripts; PDFium licence unverified) | Render parity with web; one engine | You build the viewer UI; extra dylib to sign and notarize |

**Ranking:** 1) PDFKit viewer with PDFium in the Rust core for export, batch thumbnails and any render that must match web. 2) PDFium-only viewer (parity, more UI work). 3) PDFKit-only (drift from web).

**Mitigation.** Golden-image tests across PDFKit and PDFium to measure drift on the same fixtures.

### B9. Updates, notarization and minimum macOS

- **Sparkle 2.10.1** (2026-10-25). Sparkle 2.10.0 (2026-09-13) raised the minimum deployment target to macOS 12.0 and noted it was "due to Xcode 27 dropping support for deploying to older OS versions" (Sparkle release notes). The LICENSE is MIT-style with bundled third-party notices (repo metadata NOASSERTION); bundle those notices. Sparkle docs recommend EdDSA (ed25519) and deprecate DSA. Key loss breaks updates for installed users; store the key offline.
- **Notarization.** Use `notarytool` (Apple doc). The Apple doc mentions `altool` as legacy; exact deprecation wording unverified.

**Minimum macOS floor.**

| Floor | Status on 2026-10-31 | Verdict |
|---|---|---|
| macOS 14 | End of life 2026-09-14 (endoflife.date) | Reject |
| macOS 15 | Not end-of-life (endoflife.date lists no end date) | 1. Recommended floor |
| macOS 26 | Current OS with Foundation Models | 2. Use only if owner analytics show a clear majority on 26+ |

**Market share (unverified).** TelemetryDeck reports 86.0% Tahoe for the week of 2026-07-27 in a single secondary article. BusinessStats models about 64% for mid-2026. StatCounter lumps releases since Catalina and cannot isolate Tahoe. The owner should decide from first-party analytics.

**Foundation Models.** Apple's updates page (June 2026) lists the `LanguageModel` protocol (any on-device or server LLM), `PrivateCloudComputeLanguageModel`, image input with Vision tools, `ToolCallingMode`, `DynamicProfile`, and an updated Instruments template. Apple warns that the model changes with each OS update, so retest prompts. The exact availability floor for these APIs could not be extracted from the doc (the availability block was empty in this fetch). Gate every use with an availability check and confirm the floor against the Xcode 27 SDK. The February 2026 Apple note cites a 4,096-token window for Playground estimates; read `contextSize` at runtime rather than hardcoding it.

### B10. macOS i18n with String Catalogs

- **String Catalogs** (`.xcstrings`) are Apple's current format (Apple doc verified). Xcode exports localization files for translators; Apple does not name the format in the fetched text, but XLIFF is the standard export (unverified for Xcode 27).
- **Weblate** imports XLIFF, with review state carried across (Weblate docs). Native `.xcstrings` import in Weblate is not confirmed.

**Ranking.** 1) XLIFF round-trip through the same platform as web, with EN in git. 2) Direct `.xcstrings` import, if the platform supports it (unverified). 3) Separate macOS translation process (drift; rejected).

Keep one shared glossary and key naming across web and macOS.

## Recommended stack

| Layer | Pick | Version | Licence |
|---|---|---|---|
| Web framework | Next.js (static export) | 16.4.3 | MIT |
| UI runtime | React | 19.3.1 | MIT |
| Language | TypeScript | 7.0.2 | Apache-2.0 |
| Styling | Tailwind CSS | 4.3.3 | MIT |
| UI primitives | @base-ui/react | 1.9.0 | MIT |
| i18n | next-intl | 4.14.9 | MIT |
| Translation platform | Weblate (self-hosted) | 2026.10.1 | GPL-3.0 (server only) |
| Web PDF viewer and editing | @embedpdf/react-pdf-viewer and plugins | 2.15.1 | Apache-2.0 per repo (npm says MIT) |
| PDF fallback | pdfjs-dist | 6.4.299 | Apache-2.0 |
| Rich-text editor | @tiptap/react (with table and collaboration) | 3.32.2 | MIT |
| Collaboration server | @hocuspocus/server | 4.8.0 | MIT |
| Page DnD | @atlaskit/pragmatic-drag-and-drop | 4.0.0 | Apache-2.0 |
| Virtual lists | @tanstack/react-virtual | 3.14.24 | MIT |
| Animation | motion | 14.9.0 | MIT |
| Command palette | cmdk | 1.1.1 | MIT |
| State | zustand | 5.0.16 | MIT |
| Lint and format | @biomejs/biome | 2.6.4 | MIT OR Apache-2.0 |
| Unit tests | vitest | 5.0.5 | MIT |
| E2E tests | @playwright/test | 1.64.1 | Apache-2.0 |
| CI runtime | Node.js | 26.13.0 (LTS, released 2026-10-28) | MIT (standard licence; not re-verified) |
| macOS toolchain | Xcode / Swift | 27.2 (27B52) / 6.4 | Apple SDK terms |
| Rust to Swift bindings | uniffi (plus own bindgen binary) | 0.32.2 | MPL-2.0 (needs cargo-deny exception) |
| Native PDF core | pdfium-render (PDFium binary) | 0.9.4 | MIT OR Apache-2.0 (PDFium upstream unverified) |
| macOS viewer | PDFKit | System framework | Apple SDK |
| Auto-update | Sparkle | 2.10.1 | MIT-style with bundled notices |
| Notarization | notarytool | Xcode 27.2 toolchain | Apple SDK |
| macOS i18n | String Catalogs with XLIFF round-trip | Xcode 27.2 | Apple SDK |
| Deployment floor | macOS | 15 floor; AI gated at 26+ | n/a |

## Top 5 risks

1. **Licence gate.** BlockNote (MPL-2.0 core; GPL/proprietary XL packages) is excluded. UniFFI (MPL-2.0) needs a per-crate cargo-deny exception. EmbedPDF's npm MIT metadata conflicts with its repo Apache-2.0 declaration. Resolve each licence from its LICENSE file before release, and confirm PDFium's upstream licence text.
2. **Static export limits.** COOP/COEP and any other headers must be set by the host, because static export ignores `headers` in next.config. Locale detection by proxy and default image optimisation are unavailable. Choose the host early and add a Playwright check that `crossOriginIsolated` is true.
3. **Native FFI maturity.** Async UniFFI code is not `Sendable`, and Rust panics in non-throwing calls are fatal. cargo-swift lags behind UniFFI. Mitigation: pin uniffi 0.32.2, use the bindgen binary directly, catch panics at the boundary, and wrap async results in actors.
4. **Render parity and PDFKit quality.** PDFium on web and PDFKit on macOS will render differently, and the Tahoe PDFKit issues are anecdotal, with no macOS 27 status. Mitigation: golden-image tests across both renderers on the same fixtures, and PDFium in the Rust core for export.
5. **Staleness and young components.** cmdk's last release was in March 2025 and dnd-kit's last npm release in December 2024. Lexical and EmbedPDF are on pre-2.x or 0.x cadences. Foundation Models changes with each OS update. Mitigation: pin exact versions, keep swap points (cmdk to Ariakit; Pragmatic DnD already chosen over dnd-kit), and retest AI prompts each OS release.

## Licence gate check (against AGENTS.md)

- Pass: Next.js, React, TypeScript, Tailwind, Base UI, next-intl, EmbedPDF (Apache-2.0 per repo), pdfjs-dist, Tiptap core and table and collaboration, Pragmatic DnD, TanStack Virtual, motion, Zustand, Biome, Vitest, Playwright, pdfium-render, Sparkle (with bundled notices).
- Needs a decision: uniffi (MPL-2.0 Rust dependency, requires a cargo-deny per-crate exception); PDFium binary notice text (to confirm).
- Fail: BlockNote (MPL-2.0 JS core, which the JS allow-list does not cover, plus GPL/proprietary XL packages).
- Tooling outside the artifact: Weblate (GPL-3.0 server). Acceptable only if it never ships with the app.

## Limitations

- No 1,000-page benchmark was run. Performance claims are from vendor pages or single third-party reports and are marked unverified.
- Market-share figures come from secondary sources and conflict. The owner should use first-party analytics.
- Swift 6.4 concurrency changes and Xcode 27 SDK details were checked against release notes only.
- Paraglide and Lingui Next static-export wiring were not built. The next-intl static path comes from its docs, not a running POC.
- Weblate's Git workflow, its `.xcstrings` support and Tolgee's sync commands were not re-verified in this run.
- Apple's Foundation Models availability floor could not be extracted from the doc fetch.
- Slate's maturity status and the upstream PDFium licence text were not verified.

## Unresolved questions

1. **Minimum macOS target.** Is the floor 15 or 26? This depends on the owner's first-party analytics. The recommendation is 15 with AI gated at 26+.
2. **AI scope.** Is Foundation Models-based AI a core feature or optional? A core feature pushes the floor toward 26.
3. **GPL server.** Is a self-hosted GPL-3.0 Weblate acceptable as build and translation tooling outside the shipped artifact?
4. **EmbedPDF licence.** Confirm the LICENSE and `package.json` of the React viewer and each plugin before shipping, since npm reports MIT and the repo reports Apache-2.0.
5. **PDFium binary licence.** Which notice text must ship with the WASM and dylib builds?
6. **Weblate xcstrings.** Does Weblate import `.xcstrings` directly? Test with a sample catalog, or fall back to XLIFF.
7. **Translation budget.** Is a hosted translation plan acceptable, or must the platform be self-hosted with ops cost?
8. **Reflow editor scope.** Is the "Word-like" mode a reflow of extracted text or a true page-layout editor? This changes the editor choice and the data model.
