# Cloud hardening follow-ups

Status: Delivered through [PR #45](https://github.com/bavanchun/AriadShift/pull/45), merged into `dev` on 2026-10-24. The final head was `0807a29`, rebased on `df71a6b`; the original branch was `fix/cloud-hardening-2`. The dependency advisory was resolved on `dev` before final delivery and is no longer a blocker.

## Outcome and constraints

Close the seven low findings from merged PR #44 with real-service/process evidence. Preserve upload and conversion contracts, restrict edits to server, infrastructure and documentation surfaces, serialize heavy commands, and leave merge order outside scope. No protected branch, release or repository setting is changed.

## Delivered behavior

- Configurable fair upload-size sweeps include completed inputs and retired signed-URL replays; workers delete inputs that fail download size/hash verification.
- An inherited helper network filter blocks non-UNIX sockets and `io_uring_setup` before threads start, without changing the host sandbox.
- The read-only container smoke converts Markdown to DOCX through Pandoc in required mode and verifies worker/supervisor procfs protection.
- Workers require an explicit private work directory; lifetime locks reject symlinks, foreign owners and nonregular files.
- Restricted API/worker logins run the upload, plan, DOCX job and download path. Unused worker table grants were removed.
- The procfs test uses shell tools; Docling workspace/container capacity is documented from real-pack measurements and code limits.

## Acceptance and verified delivery

[The final CI run](https://github.com/bavanchun/AriadShift/actions/runs/40083662971) passed Static checks, Verify on Linux/macOS/Windows, Web, Fuzz, Server integration, Desktop on all three OSes and CodeQL. The PR was ready and merged, not left as a blocked draft.

Server integration ran all 97 tests with zero skipped, including the real PostgreSQL/SeaweedFS flow, required-mode helper conversion and restricted-login path. It also passed the unavailable-sandbox regression and the read-only Pandoc container smoke. The final post-rebase local verification reported 783 workspace tests and 67 web tests; earlier counts and the earlier dependency-advisory failure describe superseded runs.

SeaweedFS rejects changed bytes under its checksum-bound signed PUT with `400 BadDigest`. Tests replace the real object through the authenticated storage API to verify cleanup independently; this does not claim the store accepted a changed signed PUT. The signed Docling and Office images were only linted by the container smoke.

## Subsequent maintenance

A follow-up on `fix/cloud-hardening-3`, starting from `dev` at `c52ef98`, addresses the low findings in the PR #45 review: a successful post-expiry sweep with valid-input controls, an open-only procfs probe, helper-filter startup preflight in every mode, an explicit untested Kubernetes-fragment statement, read-only worker upload grants, and current delivery records. The sweep, startup preflight, privilege revocations and documentation are implemented; the procfs test edit awaits one-time privacy-hook approval. The Mac real-service suite passed 95 tests, and workspace clippy passed. Full local CI and hosted checks remain in progress. These follow-up results do not retroactively change the PR #45 evidence above.

## Deployment and rollback

Back up production before applying migrations, then reapply [the runtime-role script](../../infra/server-roles.sql), including its upload-write revocations. Set an explicit private `ARIAD_WORK_DIR` before deploying; missing configuration intentionally fails startup. Helper filter installation fails closed; no runtime profile is weakened. The earlier migration replaces only a partial index and changes no document data. Prefer a forward fix; an older binary refuses a database containing an unknown migration.
