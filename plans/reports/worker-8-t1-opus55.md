# Worker 8-T1 (Opus 5.5 High): desktop release checksums and Minisign signatures

Branch `feat/v1-distribution--desktop` (from `origin/dev` at `3b85281`).

## Outcome

Each `v*` tag release now gets `SHA256SUMS` over every desktop installer and updater artifact, a `.minisig` for each artifact and for `SHA256SUMS`, all signed with the existing `PACK_MINISIGN_KEY` (`docs/releasing.md` names no dedicated desktop key). The signatures are verified against the committed pack public keys before they are published. Apple signing, Apple notarization and Windows signing stay wired. Each turns on only when its whole secret group exists, is skipped with a `::notice::` when the group is absent, and fails the job when the group is partial. The Tauri updater signature flow is unchanged. Nothing was built or tested locally, as the brief requires.

## Files changed

- `.github/workflows/desktop.yml`
- `scripts/desktop-release-sums.mjs` (new): collects desktop assets from the GitHub release (or from a local `target/release/bundle` with `--from`) and writes `SHA256SUMS`.
- `scripts/desktop-release-sums.test.mjs` (new): `node:test` unit tests for selection, required kinds, checksum format and local collection.
- `scripts/desktop-release-sign.sh` (new): signs with Minisign, verifies each signature against the first matching public key it is given, and runs `sha256sum --check`.

## Commits

- `062d371` ci(desktop): attach signed SHA256SUMS to desktop releases
- `ba98667` ci(desktop): fail closed on partial platform signing secrets
- plus the commit that adds this report

## What the workflow does now

Release path (tag push), in order:

1. `release` (matrix, unchanged except as listed here). A new **Check platform signing configuration** step gets only the presence booleans (`secrets.X != ''`), never the values. It fails on a partial Apple certificate group, a partial notarization group, notarization configured without signing, or a partial Windows group. When a group is absent it emits a notice. Its outputs `apple`, `notarize` and `windows` gate the Windows certificate import and two new post-build checks: `codesign --verify --deep --strict` (plus `stapler validate` and `spctl --assess` when notarized) on macOS, and `Get-AuthenticodeSignature` = `Valid` on every NSIS/MSI installer on Windows. Apple secrets now reach `tauri-action` only on the macOS runner. The job-level `HAS_WINDOWS_CERTIFICATE` env was removed.
2. `verify-release` (unchanged): verifies `latest.json` updater signatures.
3. `sign-release` (new, `environment: release`, workflow default `contents: read`): runs `node scripts/desktop-release-sums.mjs`. The script downloads every release asset whose name starts with the Tauri `productName` (`AriadShift`) and ends with an installer or updater suffix (`.AppImage`, `.deb`, `.rpm`, `.dmg`, `.app.tar.gz`, `-setup.exe`, `.msi`, each optionally `.sig`), plus `latest.json`. It requires at least one of each kind and `latest.json`, and cross-checks every download against the `digest` GitHub records for the asset. Then `scripts/desktop-release-sign.sh` signs with `PACK_MINISIGN_KEY` (only that step sees it) and verifies against `crates/ariad-host/data/pack-keys/*.pub`. The step fails closed if the key is empty or matches no committed key. Only `SHA256SUMS` and `*.minisig` are uploaded as the `desktop-release-signatures` workflow artifact.
4. `publish-checksums` (new, `contents: write`, no secrets): downloads that artifact and runs `gh release upload --clobber`, so a re-run is idempotent.

The trusted comment is `desktop:AriadShift release:vX.Y.Z file:<name>`. It never contains a `pack:` token, so `ariad-host`'s pack verifier (`check_trusted_comment`) rejects a desktop signature even though the same key signed it. Tauri `.sig` files are listed in `SHA256SUMS` but are not signed again.

PR path: Linux installs `minisign`. Every OS runs `node --test scripts/desktop-release-sums.test.mjs`. After `just desktop-build`, Linux rehearses the full flow with a throwaway unencrypted key, then checks that the sign script refuses that key against the committed pack public keys.

Lint run locally (not builds or tests): `actionlint` 1.7.12 with shellcheck, `zizmor` 1.30.1 (no findings), `shellcheck` on the sign script, `typos`, `biome check` on the new `.mjs` files, and `scripts/check-commits.sh origin/dev..HEAD`. All passed. Actions reuse the SHAs already pinned in the repository (`upload-artifact` v7.0.2, `download-artifact` v8.0.2, `checkout` v7.0.1, `setup-node` v7.1.0).

## Verification commands for the coordinator

```sh
just lint-workflows
node --test scripts/desktop-release-sums.test.mjs
pnpm exec biome check scripts/desktop-release-sums.mjs scripts/desktop-release-sums.test.mjs
uvx --from shellcheck-py shellcheck scripts/desktop-release-sign.sh
just desktop-build
# Local bundle verifies against its checksum and Minisign signature (Linux, needs minisign).
work=$(mktemp -d)
minisign -G -W -p "$work/k.pub" -s "$work/k.key"
node scripts/desktop-release-sums.mjs --from "${CARGO_TARGET_DIR:-target}/release/bundle" "$work/release"
MINISIGN_SECRET_KEY="$(cat "$work/k.key")" bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 "$work/k.pub"
# Must fail: the throwaway key matches no committed pack key.
! MINISIGN_SECRET_KEY="$(cat "$work/k.key")" bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 crates/ariad-host/data/pack-keys/*.pub
rm -rf "$work"
```

The PR run of `desktop.yml` exercises the rehearsal step on `ubuntu-26.04`. The release jobs only run on a `v*` tag.

## Contract with `release.yml` (owned by T2)

- No artifacts are handed to `release.yml`. The desktop workflow still waits for the release that `release.yml`'s `host` job creates, and attaches its own assets to it.
- The desktop workflow owns these release asset names: `SHA256SUMS`, `SHA256SUMS.minisig` and `<desktop asset>.minisig`. `release.yml` must not upload a file with those names, because `--clobber` would replace it. CLI archives keep their per-archive `.sha256` files. If T2 wants one checksum file for the CLI, it should use a different name (for example `ariad-cli-SHA256SUMS`).
- `release.yml` must not upload assets whose names start with `AriadShift`, or they would be picked up as desktop assets.

## Docs changes needed elsewhere (T3/T4)

- `docs/desktop.md` "Updates and release configuration" and the new `docs/install.md` should cover verification: `sha256sum --check --ignore-missing SHA256SUMS` (macOS: `shasum -a 256 --check --ignore-missing SHA256SUMS`), then `minisign -Vm <file> -p crates/ariad-host/data/pack-keys/d87b0e33f55ea02b.pub` (or `-P <base64 key>`). The trusted comment shows `desktop:AriadShift release:vX.Y.Z file:<name>`. Also document that partial Apple or Windows secret groups now fail the release.
- `docs/releasing.md`: `PACK_MINISIGN_KEY` (and optional `PACK_MINISIGN_KEY_PASSWORD`) now also signs desktop releases. Add the `sign-release` and `publish-checksums` jobs to the workflow description and the failure table: a key mismatch or missing desktop kind fails `sign-release`, and a re-run is safe. The environment secret list there currently names only the tap and crates tokens.
- `docs/ci.md`, if it lists desktop jobs.

## Open concerns

- **Pack key reuse.** Desktop artifacts and pack manifests share one key, as the brief directs. The trusted comments keep the two domains apart, but a leak of the key would affect both. The production key check in phase 3 C1 (the secret matches `d87b0e33f55ea02b.pub`) gates this job: on a mismatch `sign-release` fails closed.
- **Key password.** The `release` environment has no password secret for `PACK_MINISIGN_KEY`, so I assume the key was generated unencrypted (`minisign -G -W`). If it is encrypted, the maintainer must add `PACK_MINISIGN_KEY_PASSWORD`; until then signing fails closed with empty password input. The secret may be stored as the full `minisign -G` file or as the bare base64 line; the script accepts both.
- **Checks after upload.** The platform signature checks run after `tauri-action` has already uploaded to the public release, because that action builds and uploads in one step. The secret-group check does run before the build. If a post-build check fails, `sign-release` and `publish-checksums` do not run, so the release visibly lacks `SHA256SUMS`.
- **Windows certificate chain.** The Authenticode check requires `Valid`, which needs a certificate chained to a trusted root. A self-signed test certificate would fail it, which is intended.
- **Inferred asset names.** The `.rpm` kind and the `AriadShift` prefix come from Tauri's `targets: "all"` and from tauri-action's `getAssetName` (basename, and `<productName>_<version>_<arch>.app.tar.gz` on macOS). I have not seen them on a real tagged release. If a kind is missing, `sign-release` fails with the list of what is missing.

Status: DONE_WITH_CONCERNS
