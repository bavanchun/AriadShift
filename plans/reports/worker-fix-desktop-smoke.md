# Desktop smoke locator

## Cause and evidence

The second conversion waits for `//button[normalize-space()='Convert']`.
The desktop navigation button gained a child keyboard hint in commit 874a19d5,
so its full XPath string value includes `Ctrl+1`; the label itself remains in a
`span`. The first Markdown conversion succeeds, but the test cannot return
from Queue to Convert to start the DOCX conversion.

The failing PR run [41388022895, job 125130086280](https://github.com/bavanchun/AriadShift/actions/runs/41388022895/job/125130086280) shows the exact failing locator, a rendered Queue page,
`All done · 1 converted`, and the Markdown output path. This rules out a blank
webview or an absent Convert tab as the cause of that failure. EGL/DRI3 and
connection-reset warnings are present but do not establish a rendering failure.

The requested dev commit `9f13595f` has no Desktop workflow run: its push
runs are CI, CodeQL, and Cloud E2E. As a comparable earlier passing PR run,
[40784863931, job 123155395189](https://github.com/bavanchun/AriadShift/actions/runs/40784863931/job/123155395189)
ran the same smoke against the previous plain-text navigation markup. Both
jobs used runner image `20261025.179.1`, WebKitGTK
`2.52.6-0ubuntu0.26.04.1`, and tauri-driver `2.1.0`; both log the DRI3 warning.
Both built the packaged app successfully. Release compilation took 9m15s in
the passing job and 10m22s in the failing job. The passing smoke finished in
about 10 seconds; the failing smoke timed out after about 35 seconds, including
its 30-second element wait. The common selector was valid before the UI change.

## Fix and verification

Scope the navigation locator to the Convert label span. The smoke still drives
the packaged app through Markdown and Pandoc-generated DOCX conversions and
checks the contents of each HTML output. No app behavior or public contract
changes. The existing end-to-end smoke is the regression test.

- PASS: Python AST parsing of `scripts/desktop-smoke.py`.
- PASS: `xmllint --xpath` comparison on the current navigation structure:
  old locator matched zero, new locator matched one.
- PASS: `git diff --check`.
- Python lint: the repository's Ruff recipe covers `engines/docling`, not
  `scripts`; no script-specific Python lint was found.
- Local packaged-app smoke was not run: tauri-driver, WebKitWebDriver, Xvfb,
  and Pandoc are unavailable locally; the build would be heavy on a shared
  machine. CI is the integration check.
- PASS: [Desktop workflow 41403480223, Ubuntu job 125140213242](https://github.com/bavanchun/AriadShift/actions/runs/41403480223/job/125140213242). The packaged-app smoke reports both Markdown and Pandoc DOCX to HTML passed; the packaged PDF/Office engine smoke also passed.
- `sh scripts/check-commits.sh origin/dev..HEAD` could not start because the
  worktree lacks `.tools/bin/committed`; the main checkout's committed binary
  was used against the same config and commits instead.

Assumption: the manager will create the pull request, as required by the common
worker rules. This worker is explicitly permitted to push the branch.

## Suggested PR text

Title: `fix(desktop): repair the packaged app smoke locator`

Body: `Fixes #179. Select the Convert navigation label independently of its
keyboard shortcut, retaining both real Markdown and DOCX conversion checks.`
