# Distribution review

No introduced critical or high findings were verified in the requested diff.
The source review is mergeable within that scope; coordinator verification is
still required, and an inherited guard-suite problem is recorded below.

## Scope and evidence

- Reviewed `git diff origin/dev...origin/feat/v1-distribution` at base
  `e0f68a820c457ee729ee64b748e21097ce719d0a` and head
  `1790d4546212a3d6d695b7a388e8955f3d0f522e`. The review worktree initially
  matched that head and was clean. The remote distribution head was also
  checked with `git ls-remote` and matched.
- Read the common ASUS brief, review brief, repository instructions, Git
  workflow, completion plan and distribution phase, architecture distribution
  boundaries, all changed implementation and documentation, regression tests,
  desktop configuration and updater verification, and the three delivery
  reports. Applied the installed `ak-code-review` skill.
- No builds, tests, workflow dispatches, releases or registry operations were
  run, as required by the ASUS brief. `git diff --check` passed for the reviewed
  diff. Runtime success is not claimed.

## Critical and high findings

None introduced by this diff.

## Checked release contracts

| Contract | Source evidence and result |
|---|---|
| Unsigned desktop distribution remains supported. | `.github/workflows/desktop.yml:170` checks secret presence before building. Empty Apple and Windows groups emit notices; partial groups fail. Unsigned first-run instructions appear in `docs/install.md:215` and `docs/desktop.md:24`. |
| Configured platform signing does not silently turn into a successful unsigned run. | `.github/workflows/desktop.yml:233` imports a configured Windows certificate. Platform verification at lines 285 and 296 fails the job on invalid signatures, and configured Apple notarization also checks stapling and assessment. These checks happen after the action uploads; publication is not transactional. |
| Desktop assets receive checksums and detached signatures. | `scripts/desktop-release-sums.mjs:35` selects the configured product's installer and updater kinds plus `latest.json`, including Tauri `.sig` files. It requires all seven artifact kinds at line 118 and hashes downloaded bytes. `scripts/desktop-release-sign.sh:38` signs every listed non-`.sig` asset and the manifest, verifies against committed public keys, and checks every manifest digest. `.sig` files are covered by the signed manifest. |
| Production keys and privileged artifacts stay on the tag path. | `.github/workflows/desktop.yml:23` keeps PR checks separate from tag publishing. Release secrets are scoped to the relevant steps; checksum signing has read permission and a separate upload job has write permission. The artifact download at line 372 names an artifact from the same workflow run, without a foreign run or repository input. No new path from an untrusted PR to production signing was found. |
| Homebrew URLs and hashes derive from the built archives. | `.github/workflows/release.yml:283` retains cargo-dist generation. `scripts/release/verify-homebrew.py:26` hashes all five actual archives and compares sidecars; lines 43–65 check exact versioned release URLs, OS/CPU branches, digests, Pandoc dependency and the installed binary. Its expected structure matches the pinned upstream template. |
| Missing optional publishing tokens succeed with notices. | `.github/workflows/publish-crates.yml:18` sets an availability output; checkout, toolchain setup and publication depend on it. `.github/workflows/release.yml:448` derives only a boolean; tap checkout, artifact download and commit are conditional, and line 484 exits successfully before any push when the token is absent. Present-token publication errors remain failures. |
| Homebrew preparation cannot directly access the tap credential. | `.github/workflows/release.yml:392` prepares the formula in an unprivileged job. The actual tap token appears only in the isolated push step at line 482. The new job-level value is a presence boolean, not the token. |
| Existing guard protections remain in the changed verifier. | `scripts/check-release-workflow.sh:85` retains structural secret-location checks; lines 138, 352 and 528 add availability and verification requirements. Existing minimal permissions, action pins, checkout credential restrictions and installer integrity checks remain. An inherited mutation fixture mismatch is detailed below. |

## Open concerns and verification limits

1. **Inherited quality-gate problem, not a distribution regression.**
   `scripts/test-check-release-workflow.sh:802` and line 808 replace a tar
   extraction string containing `${DIST_ARCHIVE%.tar.xz}/dist`. The release
   workflow now contains literal archive-member paths instead, for example
   `.github/workflows/release.yml:95`. The helper at
   `scripts/test-check-release-workflow.sh:93` exits when its target string is
   missing. Source inspection therefore predicts that `just
   check-release-workflow` stops at that mutation before finishing. Both the
   mismatched fixtures and literal paths are already present in `origin/dev`;
   this diff changes neither anchor. The coordinator should repair these
   fixtures separately and execute the full guard suite. No test was run here.
2. **Small documentation mismatch, below the requested finding severity.**
   `docs/install.md:209` names the macOS updater archive
   `AriadShift_<version>_aarch64.app.tar.gz`. The pinned action instead uploads
   `AriadShift_aarch64.app.tar.gz` and its `.sig`: its input bundle is
   `AriadShift.app.tar.gz` and its default name adds only the architecture.
   The documented DMG installer name is correct; the checksum collector's
   suffix matching and in-app metadata still accept the actual updater name.
   Correct the table without treating this as a broken installation channel.
   See the pinned upstream [build artifact paths](https://github.com/tauri-apps/tauri-action/blob/84b9d35b5fc46c1e45415bdb6144030364f7ebc5/src/build.ts#L105-L131)
   and [asset naming](https://github.com/tauri-apps/tauri-action/blob/84b9d35b5fc46c1e45415bdb6144030364f7ebc5/src/utils.ts#L80-L125).
3. GHCR and Compose implementation are absent from this branch. The copied
   cloud image delivery report agrees with the six image names documented in
   `docs/install.md:415`, but it is not live publication evidence. Validate the
   owning cloud branch and signed images after integration. The verdict does
   not certify that sibling work or complete the version-bump/docs-sweep tasks.

## Coordinator verification

Run sequentially in the shared build queue, from the integrated checkout:

```sh
node --test scripts/desktop-release-sums.test.mjs
python3 scripts/release/test-verify-homebrew.py
just check-release-workflow
just lint-workflows
just package-check
just ci
just desktop-ci
just desktop-build
```

After the local Linux desktop build, with Minisign installed:

```sh
work=$(mktemp -d)
minisign -G -W -p "$work/rehearsal.pub" -s "$work/rehearsal.key"
node scripts/desktop-release-sums.mjs --from "${CARGO_TARGET_DIR:-target}/release/bundle" "$work/release"
MINISIGN_SECRET_KEY="$(cat "$work/rehearsal.key")" bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 "$work/rehearsal.pub"
if MINISIGN_SECRET_KEY="$(cat "$work/rehearsal.key")" bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 crates/ariad-host/data/pack-keys/*.pub; then
  echo 'ERROR: mismatched signing key was accepted' >&2
  exit 1
fi
rm -rf "$work"
```

For a release PR rehearsal, download its actual workflow artifacts and run
`python3 scripts/release/verify-homebrew.py target/distrib` using the procedure
in [releasing.md](../../docs/releasing.md#verify-the-formula-without-publishing).
At the separately authorized release, inspect missing-token notices and verify
all published desktop assets against the signed manifest. Exercise configured
platform-signing failures before relying on certificate-backed distribution.

Upstream source checks used the pinned [cargo-dist 0.33.0 formula template](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/cargo-dist/templates/installer/homebrew.rb.j2),
Tauri CLI 2.12.4 [DEB naming](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/linux/debian.rs#L56-L62)
and [RPM naming](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/linux/rpm.rs#L18-L41),
and [shasum verification options](https://perldoc.perl.org/shasum).

Unresolved product questions: none. Coordinator validation remains pending.

Verdict: MERGEABLE
