# Web foundation integration review

Date: 2026-10-23. PR: [#36](https://github.com/bavanchun/AriadShift/pull/36).

## Independent review

Claude Sonnet 5.5 at `xhigh` completed a read-only review of the newly captured PR diff and repository source after integration with signed engine packs. Reviewed head: `2d980d03e2f0b327c51b2a45e8f595d82d37ae0f`; base: `c24cf9772fc6bba82af570431b2d6fa0aff0f30c`. The reviewer returned no High findings, one Medium and four Low findings. It did not run tests. The earlier review and its resolved findings remain in [web-foundation-review.md](./web-foundation-review.md).

| Finding | Resolution |
|---|---|
| Medium: incorrect Homebrew tap command and unpublished formula presented as available | Fixed in `c79a26b`: the download page uses `brew install bavanchun/tap/ashift` and states that the formula is not yet published, matching README. Verified in production HTML. |
| Low: unsupported PDF input promises a CLI route that does not exist | Fixed in `508991e`: offer the CLI only when inspection reports a reachable route elsewhere. Added a browser regression. |
| Low: dropping a file outside the active picker can navigate away | Fixed in `508991e`: window listeners cancel file drag/drop defaults, including when the picker is hidden; ordinary text drags remain unchanged. Added a browser regression. |
| Low: a failed stale-folder sweep selects the memory fallback | Retained the conservative fallback. Startup promises stale-folder cleanup; ignoring its failure could leave stored files behind while claiming OPFS readiness. The UI visibly reports the fallback. The reviewer found no reliable trigger or data loss; this is a non-blocking recovery trade-off. |
| Low: CI documentation omits JavaScript-only trigger paths | Fixed in `4f2673a`: documentation includes the brand, Node version and license-checker paths, with their actual job classification. |

There are no unresolved High or Medium findings. The reviewer marked development service-worker cache staleness as speculative; it was not demonstrated. Future browser-engine cache namespaces remain part of the heavy-route integration. Static export and its generated isolation headers were checked manually; no automated export or CI-scope classification suite was added.

## Integration and verification

- Both the engine-pack SHA-256 development profile and the WASM release profile are retained. Pack installation, environment-aware runner APIs and `EngineRow.pack` are unchanged from the base. Main and fuzz Cargo lockfiles, the pnpm lockfile and `THIRD_PARTY_LICENSES` were regenerated or checked with their owning tools.
- Integrated fuzzing exposed an existing Markdown nesting-bound defect: an empty container or a table could emit IR one level beyond the configured limit. `3a1cfd5` counts every container before checking its depth. Two boundary regressions failed before the fix and pass after it. The exact CI crash input replays successfully, and a fresh 60-second Markdown fuzz run passed 110,182 iterations.
- Linux production Lighthouse initially missed the performance bar. Separate Suspense boundaries for the theme control and converter reduced hydration blocking; the runner also awaits Chrome shutdown between audits. Pinned Lighthouse 13.5.0 now reports mobile Performance 99 for `/` and 96 for `/app`, desktop Performance 100 for both, and 100 in every other category. See [the measured table](../../../docs/web.md#lighthouse).
- The first integrated Web job timed out while downloading Ubuntu system libraries from the Azure mirror. The Web job now uses Ubuntu's primary HTTPS archive for that mirror, preserving package signatures and the existing timeout.

Final verification evidence and the completion CI link are recorded in the [phase completion notes](../phase-06-web-foundation.md#completion-verification).

Status: DONE
Summary: The independent integration review is complete; its Medium finding is fixed and no High or Medium finding remains open.
