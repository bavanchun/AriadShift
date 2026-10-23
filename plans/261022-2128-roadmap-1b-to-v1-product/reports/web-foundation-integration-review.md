# Web foundation integration review

Date: 2026-10-23. PR: [#36](https://github.com/bavanchun/AriadShift/pull/36).

## Independent review

Claude Sonnet 5.5 at `xhigh` completed a read-only review of the newly captured PR diff and repository source after integration with signed engine packs. Reviewed head: `2d980d03e2f0b327c51b2a45e8f595d82d37ae0f`; base: `c24cf9772fc6bba82af570431b2d6fa0aff0f30c`. The reviewer returned no High findings, one Medium and four Low findings. It did not run tests. The earlier review and its resolved findings remain in [web-foundation-review.md](./web-foundation-review.md).

| Finding | Resolution |
|---|---|
| Medium: incorrect Homebrew tap command and unpublished formula presented as available | Fixed: the download page uses `brew install bavanchun/tap/ashift` and states that the formula is not yet published, matching README. Verified in production HTML. |
| Low: unsupported PDF input promises a CLI route that does not exist | Fixed: offer the CLI only when inspection reports a reachable route elsewhere. Added a browser regression. |
| Low: dropping a file outside the active picker can navigate away | Fixed: window listeners cancel file drag/drop defaults, including when the picker is hidden; ordinary text drags remain unchanged. Added a browser regression. |
| Low: a failed stale-folder sweep selects the memory fallback | Retained the conservative fallback. Startup promises stale-folder cleanup; ignoring its failure could leave stored files behind while claiming OPFS readiness. The UI visibly reports the fallback. The reviewer found no reliable trigger or data loss; this is a non-blocking recovery trade-off. |
| Low: CI documentation omits JavaScript-only trigger paths | Fixed: documentation includes the brand, Node version and license-checker paths, with their actual job classification. |

There are no unresolved High or Medium findings. The reviewer marked development service-worker cache staleness as speculative; it was not demonstrated. Future browser-engine cache namespaces remain part of the heavy-route integration. Static export and its generated isolation headers were checked manually; no automated export or CI-scope classification suite was added.

## Integration and verification

- Both the engine-pack SHA-256 development profile and the WASM release profile are retained. Pack installation, environment-aware runner APIs and `EngineRow.pack` are unchanged from the base. Main and fuzz Cargo lockfiles, the pnpm lockfile and `THIRD_PARTY_LICENSES` were regenerated or checked with their owning tools.
- Earlier integrated fuzzing exposed an existing Markdown nesting-bound defect: an empty container or a table could emit IR one level beyond the configured limit. The final tree inherits the repair from `dev`, which counts every container before checking its depth; this branch adds explicit error assertions at the boundary. The earlier Linux worker replayed the CI crash input and completed a fresh 60-second Markdown fuzz run with 110,182 iterations. The current integrated CI also passes all six fuzz targets.
- Linux production Lighthouse initially missed the performance bar. Separate Suspense boundaries for the theme control and converter reduced hydration blocking; the runner also awaits Chrome shutdown between audits. Pinned Lighthouse 13.5.0 now reports mobile Performance 99 for `/` and 96 for `/app`, desktop Performance 100 for both, and 100 in every other category. See [the measured table](../../../docs/web.md#lighthouse).
- The first integrated Web job timed out while downloading Ubuntu system libraries from the Azure mirror. The Web job now uses Ubuntu's primary HTTPS archive for that mirror, preserving package signatures and the existing timeout.

## Final Mac delta review

After rebasing onto `origin/dev` at `7e2f389`, Claude Sonnet 5.5 at `xhigh` completed a fresh read-only delta review of the subsequent changes and rebase integration. It reviewed the tree at `681084b`; a review interrupted by the workspace migration was discarded and restarted from the beginning. The completed review returned zero High, zero Medium and six Low findings, and made no file changes or test runs.

The reviewer verified that the host runner, sandbox, pack environment/install root, RSS watchdog, session pool and protocol match the base. The host delta only re-exports the shared inspection types and limit. Markdown production code also matches the base; the additional regression assertions coexist with its repair. Lockfiles and the license inventory are consistent.

| Low finding | Disposition |
|---|---|
| Stale completion evidence and pre-rebase fix references | Updated this report and the phase completion notes. Historical review inputs remain identified above; resolutions use behavior and CI links instead of rebased fix SHAs. |
| A commit named as a Markdown fix now only adds tests | Retitled as `test(core): cover empty containers at nesting boundaries`. Retained its exact-error assertions. |
| Intermediate lockfile upgrades followed by reversals | Regenerated the initial WASM commit's inventory with Cargo and cargo-about, and removed the later fuzz-lock churn and reversal. The complete tree before and after this history cleanup is identical, verified with `git diff --exit-code`. |
| The unsupported-PDF browser regression depends on the current route registry | Current registry and E2E results support the assertion. Revisit it when PDF routes land; the heavy-route work must update this fixture to keep testing an input with no reachable route. The lowercase fallback format label is cosmetic. |
| File drops outside the active picker have no explicit cursor feedback | Retained the existing data-preserving behavior, covered by E2E. Cursor feedback is a non-blocking UI follow-up. |
| Lighthouse cleanup can mask an earlier error; optional chaining is redundant | Retained as diagnostic and style follow-ups. A rejected cleanup still fails the audit; the optional chaining changes no behavior. |

The reviewer did not observe GitHub checks. The controller separately verified [the complete integrated CI](https://github.com/bavanchun/AriadShift/actions/runs/39894518184), [CodeQL](https://github.com/bavanchun/AriadShift/actions/runs/39894518186) and [cross-platform release artifact builds](https://github.com/bavanchun/AriadShift/actions/runs/39894518209), all green. Its suggestion to obtain push authorization is already covered by the assignment's explicit feature-branch push grant.

The speculative Suspense regression guard is not a current defect: the reviewer confirmed that the generated HTML contains the converter and theme control. Existing reports already disclose the missing automated CI-scope classification suite. Prior Linux Lighthouse measurements remain historical measurements; this final Mac pass did not rerun Lighthouse.

Final verification evidence and the completion CI link are recorded in the [phase completion notes](../phase-06-web-foundation.md#completion-verification).

Status: DONE
Summary: The original integration review and the fresh Mac delta review are complete. No High or Medium finding remains open; non-blocking follow-ups are recorded above.
