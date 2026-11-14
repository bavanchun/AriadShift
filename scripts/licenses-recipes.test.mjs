import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const REPO_ROOT = path.resolve(import.meta.dirname, "..");

function readWasiShimLicenseText() {
  const pageText = readFileSync(
    path.join(REPO_ROOT, "apps/web/src/app/[locale]/licenses/page.tsx"),
    "utf8",
  );
  const match = /<pre[^>]*>\s*\{`([\s\S]*?)`\}\s*<\/pre>/.exec(pageText);
  if (!match?.[1]) {
    throw new Error("Could not extract WASI shim LICENSE-MIT text from licenses/page.tsx");
  }
  return match[1];
}

const WASI_SHIM_LICENSE_MIT = readWasiShimLicenseText();

const SAMPLE_CARGO_ABOUT_OUTPUT = `Third-Party Licenses for AriadShift
====================================

================================================================================

--------------------------------------------------------------------------------
License: Mozilla Public License 2.0 (MPL-2.0)
--------------------------------------------------------------------------------

Used by:
  - cssparser 0.38.0 (https://github.com/servo/rust-cssparser)
  - dtoa-short 0.3.5 (https://github.com/upsuper/dtoa-short)

License Text:
-------------
Mozilla Public License Version 2.0
==================================

================================================================================
`;

const SENTINEL_FILES = [
  "THIRD_PARTY_LICENSES",
  "apps/desktop/THIRD_PARTY_JS_LICENSES",
  "apps/web/public/notices/NOTICE.txt",
  "apps/web/public/notices/THIRD_PARTY_LICENSES.txt",
  "apps/web/public/notices/THIRD_PARTY_JS_LICENSES.txt",
];

function writeStubCargo(binDir) {
  const cargoPath = path.join(binDir, "cargo");
  writeFileSync(
    cargoPath,
    `#!/bin/sh
set -eu
case "\${1:-}" in
  about)
    case "\${2:-}" in
      --version)
        if [ "\${FAKE_CARGO_MODE:-ok}" = "missing-about" ]; then
          echo "error: no such command: \`about\`" >&2
          exit 101
        fi
        printf '%s\\n' "\${FAKE_CARGO_ABOUT_VERSION:-cargo-about 0.9.2}"
        exit 0
        ;;
      generate)
        if [ "\${FAKE_CARGO_MODE:-ok}" = "fail-generate-partial" ]; then
          printf 'partial output before failure\\n'
          echo "simulated cargo about generate failure" >&2
          exit 1
        fi
        if [ "\${FAKE_CARGO_MODE:-ok}" = "empty-generate" ]; then
          exit 0
        fi
        printf '%s' "\${FAKE_CARGO_GENERATE_STDOUT:-}"
        exit 0
        ;;
    esac
    ;;
  tree)
    printf '%s' "\${FAKE_CARGO_TREE_STDOUT:-}"
    exit 0
    ;;
esac
echo "unexpected fake cargo args: $*" >&2
exit 1
`,
    "utf8",
  );
  chmodSync(cargoPath, 0o755);
}

function createIsolatedFixture({
  withNodeModules = false,
  desktopPkgLicense = "MIT",
  webShimLicense = "MIT",
  stalePnpmLock = false,
  staleCargoLock = false,
  includeSentinels = true,
} = {}) {
  const outerDir = mkdtempSync(path.join(os.tmpdir(), "ariad-licenses-test-"));
  const fixtureDir = path.join(outerDir, "repo");
  const binDir = path.join(outerDir, "bin");
  const tmpRoot = path.join(outerDir, "tmp");

  mkdirSync(fixtureDir, { recursive: true });
  mkdirSync(binDir, { recursive: true });
  mkdirSync(tmpRoot, { recursive: true });
  writeStubCargo(binDir);

  copyFileSync(path.join(REPO_ROOT, "justfile"), path.join(fixtureDir, "justfile"));
  mkdirSync(path.join(fixtureDir, "scripts"), { recursive: true });
  for (const file of [
    "generate-licenses.mjs",
    "generate-desktop-licenses.mjs",
    "check-js-licenses.mjs",
  ]) {
    copyFileSync(path.join(REPO_ROOT, "scripts", file), path.join(fixtureDir, "scripts", file));
  }

  mkdirSync(path.join(fixtureDir, "apps/web/scripts"), { recursive: true });
  copyFileSync(
    path.join(REPO_ROOT, "apps/web/scripts/sync-notices.mjs"),
    path.join(fixtureDir, "apps/web/scripts/sync-notices.mjs"),
  );
  cpSync(
    path.join(REPO_ROOT, "apps/web/scripts/license-texts"),
    path.join(fixtureDir, "apps/web/scripts/license-texts"),
    { recursive: true },
  );
  for (const file of ["engine-assets.mjs", "pandoc-source-inventory.mjs", "pdfium-assets.mjs"]) {
    copyFileSync(path.join(REPO_ROOT, "apps/web", file), path.join(fixtureDir, "apps/web", file));
  }
  mkdirSync(path.join(fixtureDir, "apps/web/src/app/[locale]/licenses"), { recursive: true });
  copyFileSync(
    path.join(REPO_ROOT, "apps/web/src/app/[locale]/licenses/page.tsx"),
    path.join(fixtureDir, "apps/web/src/app/[locale]/licenses/page.tsx"),
  );

  mkdirSync(path.join(fixtureDir, "crates/ariad-pdf/assets/liberation"), { recursive: true });
  writeFileSync(path.join(fixtureDir, "about.hbs"), "{{#each licenses}}{{name}}{{/each}}\n");
  writeFileSync(path.join(fixtureDir, "NOTICE"), "AriadShift\nCopyright AriadShift\n");
  writeFileSync(path.join(fixtureDir, "Cargo.toml"), "[workspace]\nmembers = []\n");
  writeFileSync(
    path.join(fixtureDir, "crates/ariad-pdf/assets/glyphless-LICENSE.txt"),
    "Glyphless OCR Font License\n",
  );
  writeFileSync(
    path.join(fixtureDir, "crates/ariad-pdf/assets/liberation/LICENSE-liberation.txt"),
    "Liberation OFL-1.1\n",
  );
  writeFileSync(
    path.join(fixtureDir, "crates/ariad-pdf/assets/LICENSE-sRGB-v4.txt"),
    "sRGB v4 CC0-1.0\n",
  );

  const dtoaVer = staleCargoLock ? "0.9.9" : "0.3.5";
  writeFileSync(
    path.join(fixtureDir, "Cargo.lock"),
    `version = 4

[[package]]
name = "ammonia"
version = "4.1.0"
dependencies = [
 "cssparser 0.38.0",
]

[[package]]
name = "cssparser"
version = "0.38.0"

[[package]]
name = "dtoa-short"
version = "${dtoaVer}"
`,
  );

  const shimLockVer = stalePnpmLock ? "9.9.9" : "0.4.2";
  writeFileSync(
    path.join(fixtureDir, "pnpm-lock.yaml"),
    `lockfileVersion: '9.0'

importers:
  apps/web:
    dependencies:
      '@bjorn3/browser_wasi_shim':
        specifier: ${shimLockVer}
        version: ${shimLockVer}

packages:
  '@bjorn3/browser_wasi_shim@${shimLockVer}':
    resolution: {integrity: sha512-fixture}

snapshots:
  '@bjorn3/browser_wasi_shim@${shimLockVer}': {}
`,
  );

  mkdirSync(path.join(fixtureDir, "apps/desktop"), { recursive: true });
  writeFileSync(
    path.join(fixtureDir, "apps/desktop/package.json"),
    JSON.stringify(
      {
        name: "@ariadshift/desktop",
        private: true,
        dependencies: {
          "sample-desktop-pkg": "1.0.0",
        },
      },
      null,
      2,
    ),
  );
  writeFileSync(
    path.join(fixtureDir, "apps/web/package.json"),
    JSON.stringify(
      {
        name: "@ariadshift/web",
        private: true,
        dependencies: {
          "@bjorn3/browser_wasi_shim": "0.4.2",
        },
      },
      null,
      2,
    ),
  );

  if (withNodeModules) {
    mkdirSync(path.join(fixtureDir, "node_modules"), { recursive: true });

    const desktopPkgDir = path.join(fixtureDir, "apps/desktop/node_modules/sample-desktop-pkg");
    mkdirSync(desktopPkgDir, { recursive: true });
    writeFileSync(
      path.join(desktopPkgDir, "package.json"),
      JSON.stringify({
        name: "sample-desktop-pkg",
        version: "1.0.0",
        license: desktopPkgLicense,
      }),
    );
    writeFileSync(path.join(desktopPkgDir, "LICENSE"), "MIT License text\n");

    const webShimDir = path.join(fixtureDir, "apps/web/node_modules/@bjorn3/browser_wasi_shim");
    mkdirSync(webShimDir, { recursive: true });
    writeFileSync(
      path.join(webShimDir, "package.json"),
      JSON.stringify({
        name: "@bjorn3/browser_wasi_shim",
        version: "0.4.2",
        license: webShimLicense,
      }),
    );
    writeFileSync(path.join(webShimDir, "LICENSE-MIT"), `${WASI_SHIM_LICENSE_MIT}\n`);
  }

  if (includeSentinels) {
    mkdirSync(path.join(fixtureDir, "apps/web/public/notices"), { recursive: true });
    for (const relPath of SENTINEL_FILES) {
      writeFileSync(path.join(fixtureDir, relPath), `SENTINEL:${relPath}\n`, "utf8");
    }
  }

  return {
    outerDir,
    fixtureDir,
    binDir,
    tmpRoot,
    cleanup() {
      rmSync(outerDir, { recursive: true, force: true });
    },
  };
}

function snapshotSentinels(fixtureDir) {
  return new Map(
    SENTINEL_FILES.map((relPath) => [
      relPath,
      readFileSync(path.join(fixtureDir, relPath), "utf8"),
    ]),
  );
}

function assertSentinelsUnchanged(fixtureDir, expected) {
  for (const [relPath, content] of expected) {
    assert.equal(
      readFileSync(path.join(fixtureDir, relPath), "utf8"),
      content,
      `${relPath} must remain byte-for-byte unchanged`,
    );
  }
}

function assertNoLeftoverTempFiles(fixtureDir, tmpRoot, { expectNoticesDir = true } = {}) {
  const dirsToCheck = [fixtureDir, path.join(fixtureDir, "apps/desktop")];
  const publicDir = path.join(fixtureDir, "apps/web/public");
  if (existsSync(publicDir)) {
    dirsToCheck.push(publicDir);
  }
  for (const dir of dirsToCheck) {
    const stray = readdirSync(dir).filter(
      (name) => name.startsWith(".THIRD_PARTY") || name.startsWith(".notices-stage-"),
    );
    assert.deepEqual(stray, [], `leftover temp entries in ${dir}: ${stray.join(", ")}`);
  }
  const noticesDir = path.join(fixtureDir, "apps/web/public/notices");
  if (expectNoticesDir) {
    assert.deepEqual(readdirSync(noticesDir).sort(), [
      "NOTICE.txt",
      "THIRD_PARTY_JS_LICENSES.txt",
      "THIRD_PARTY_LICENSES.txt",
    ]);
  } else {
    assert.equal(existsSync(noticesDir), false, "apps/web/public/notices must not be created");
  }
  assert.deepEqual(readdirSync(tmpRoot), [], `leftover temp entries in ${tmpRoot}`);
}

function runJust(fixture, recipe, extraEnv = {}) {
  const env = {
    ...process.env,
    PATH: `${fixture.binDir}:${process.env.PATH ?? ""}`,
    TMPDIR: fixture.tmpRoot,
    FAKE_CARGO_GENERATE_STDOUT: SAMPLE_CARGO_ABOUT_OUTPUT,
    FAKE_CARGO_TREE_STDOUT: "cssparser v0.38.0\ndtoa-short v0.3.5\n",
    ...extraEnv,
  };
  delete env.ARIADSHIFT_ROOT_DIR;
  delete env.ARIADSHIFT_THIRD_PARTY_LICENSES;
  delete env.ARIADSHIFT_DESKTOP_LICENSES_OUT;
  return spawnSync("just", [recipe], {
    cwd: fixture.fixtureDir,
    env,
    encoding: "utf8",
  });
}

test("just licenses, desktop-licenses, and web-licenses fail without node_modules and leave tracked files untouched", () => {
  const fixture = createIsolatedFixture({ withNodeModules: false });
  try {
    const initial = snapshotSentinels(fixture.fixtureDir);

    for (const recipe of ["desktop-licenses", "web-licenses", "licenses"]) {
      const result = runJust(fixture, recipe);
      assert.notEqual(
        result.status,
        0,
        `${recipe} must exit non-zero when node_modules is missing`,
      );
      const combined = `${result.stdout}\n${result.stderr}`;
      assert.match(
        combined,
        /run 'pnpm install' first/,
        `${recipe} output must instruct user to run 'pnpm install' first; got:\n${combined}`,
      );
      assertSentinelsUnchanged(fixture.fixtureDir, initial);
      assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot);
    }
  } finally {
    fixture.cleanup();
  }
});

test("missing node_modules does not create apps/web/public/notices and ARIADSHIFT_ROOT_DIR isolation works", () => {
  const fixture = createIsolatedFixture({ withNodeModules: false, includeSentinels: false });
  try {
    for (const scriptRel of [
      "scripts/generate-desktop-licenses.mjs",
      "apps/web/scripts/sync-notices.mjs",
    ]) {
      const result = spawnSync(process.execPath, [path.join(REPO_ROOT, scriptRel)], {
        cwd: REPO_ROOT,
        env: {
          ...process.env,
          ARIADSHIFT_ROOT_DIR: fixture.fixtureDir,
          TMPDIR: fixture.tmpRoot,
        },
        encoding: "utf8",
      });
      assert.notEqual(result.status, 0);
      assert.equal(result.stdout, "");
      assert.match(result.stderr, /run 'pnpm install' first/);
      assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot, { expectNoticesDir: false });
    }
  } finally {
    fixture.cleanup();
  }
});

test("missing or mismatched cargo-about fails upfront with empty stdout and leaves tracked files untouched", () => {
  const fixture = createIsolatedFixture({ withNodeModules: true });
  try {
    const initial = snapshotSentinels(fixture.fixtureDir);

    // Direct call with empty PATH (no cargo executable at all).
    const noCargo = spawnSync(
      process.execPath,
      [path.join(fixture.fixtureDir, "scripts/generate-licenses.mjs")],
      {
        cwd: fixture.fixtureDir,
        env: { PATH: "" },
        encoding: "utf8",
      },
    );
    assert.notEqual(noCargo.status, 0);
    assert.equal(noCargo.stdout, "");
    assert.match(noCargo.stderr, /cargo-about 0\.9\.2/);

    for (const extraEnv of [
      { FAKE_CARGO_MODE: "missing-about" },
      { FAKE_CARGO_ABOUT_VERSION: "cargo-about 0.8.0" },
    ]) {
      const direct = spawnSync(
        process.execPath,
        [path.join(fixture.fixtureDir, "scripts/generate-licenses.mjs")],
        {
          cwd: fixture.fixtureDir,
          env: {
            ...process.env,
            PATH: `${fixture.binDir}:${process.env.PATH ?? ""}`,
            ...extraEnv,
          },
          encoding: "utf8",
        },
      );
      assert.notEqual(direct.status, 0);
      assert.equal(direct.stdout, "", "generate-licenses.mjs must write nothing to stdout on fail");
      assert.match(direct.stderr, /cargo-about 0\.9\.2/);

      const justRes = runJust(fixture, "licenses", extraEnv);
      assert.notEqual(justRes.status, 0);
      assert.match(justRes.stderr, /cargo-about 0\.9\.2/);
      assertSentinelsUnchanged(fixture.fixtureDir, initial);
      assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot);
    }
  } finally {
    fixture.cleanup();
  }
});

test("mid-run generator failures leave all tracked notice files untouched and clean up temp files", () => {
  // 1. cargo about generate fails after emitting partial stdout, or emits empty output.
  for (const mode of ["fail-generate-partial", "empty-generate"]) {
    const fixture = createIsolatedFixture({ withNodeModules: true });
    try {
      const initial = snapshotSentinels(fixture.fixtureDir);
      const direct = spawnSync(
        process.execPath,
        [path.join(fixture.fixtureDir, "scripts/generate-licenses.mjs")],
        {
          cwd: fixture.fixtureDir,
          env: {
            ...process.env,
            PATH: `${fixture.binDir}:${process.env.PATH ?? ""}`,
            FAKE_CARGO_MODE: mode,
          },
          encoding: "utf8",
        },
      );
      assert.notEqual(direct.status, 0);
      assert.equal(direct.stdout, "");

      const justRes = runJust(fixture, "licenses", { FAKE_CARGO_MODE: mode });
      assert.notEqual(justRes.status, 0);
      assertSentinelsUnchanged(fixture.fixtureDir, initial);
      assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot);
    } finally {
      fixture.cleanup();
    }
  }

  // 2. Desktop dependency has an unapproved license -> desktop-licenses and licenses leave all files untouched.
  {
    const fixture = createIsolatedFixture({
      withNodeModules: true,
      desktopPkgLicense: "GPL-3.0-only",
    });
    try {
      const initial = snapshotSentinels(fixture.fixtureDir);
      for (const recipe of ["desktop-licenses", "licenses"]) {
        const res = runJust(fixture, recipe);
        assert.notEqual(res.status, 0);
        assert.match(res.stderr, /Unapproved license: sample-desktop-pkg/);
        assertSentinelsUnchanged(fixture.fixtureDir, initial);
        assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot);
      }
    } finally {
      fixture.cleanup();
    }
  }

  // 3. Web notices fail mid-run (stale pnpm-lock.yaml, unapproved web license, or stale Cargo.lock)
  //    after Rust & desktop succeed -> all three target areas still stay byte-for-byte unchanged.
  for (const [opts, expectedError] of [
    [{ stalePnpmLock: true }, /pnpm-lock\.yaml/],
    [{ webShimLicense: "GPL-3.0-only" }, /Unapproved license: @bjorn3\/browser_wasi_shim/],
    [{ staleCargoLock: true }, /Cargo\.lock/],
  ]) {
    const fixture = createIsolatedFixture({ withNodeModules: true, ...opts });
    try {
      // Keep a valid-but-sentinel root THIRD_PARTY_LICENSES so standalone `just web-licenses`
      // advances past Rust parsing to the JS/lockfile checks while `just licenses` proves it
      // never overwrites the sentinel marker on failure.
      writeFileSync(
        path.join(fixture.fixtureDir, "THIRD_PARTY_LICENSES"),
        `${SAMPLE_CARGO_ABOUT_OUTPUT}\nSENTINEL_MARKER\n`,
        "utf8",
      );
      const initial = snapshotSentinels(fixture.fixtureDir);
      for (const recipe of ["web-licenses", "licenses"]) {
        const res = runJust(fixture, recipe);
        assert.notEqual(res.status, 0);
        assert.match(res.stderr, expectedError);
        assertSentinelsUnchanged(fixture.fixtureDir, initial);
        assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot);
      }
    } finally {
      fixture.cleanup();
    }
  }
});

test("just licenses replaces all targets atomically on full success with zero leftover temp files", () => {
  const fixture = createIsolatedFixture({ withNodeModules: true });
  try {
    const res = runJust(fixture, "licenses");
    assert.equal(res.status, 0, `expected exit 0, stderr:\n${res.stderr}`);
    assertNoLeftoverTempFiles(fixture.fixtureDir, fixture.tmpRoot);

    const rootNotices = readFileSync(path.join(fixture.fixtureDir, "THIRD_PARTY_LICENSES"), "utf8");
    assert.match(rootNotices, /cssparser 0\.38\.0/);
    assert.match(rootNotices, /Glyphless OCR Font License/);

    const desktopNotices = readFileSync(
      path.join(fixture.fixtureDir, "apps/desktop/THIRD_PARTY_JS_LICENSES"),
      "utf8",
    );
    assert.match(desktopNotices, /sample-desktop-pkg@1\.0\.0/);

    const webRustNotices = readFileSync(
      path.join(fixture.fixtureDir, "apps/web/public/notices/THIRD_PARTY_LICENSES.txt"),
      "utf8",
    );
    assert.match(webRustNotices, /cssparser 0\.38\.0/);

    const webJsNotices = readFileSync(
      path.join(fixture.fixtureDir, "apps/web/public/notices/THIRD_PARTY_JS_LICENSES.txt"),
      "utf8",
    );
    assert.match(webJsNotices, /@bjorn3\/browser_wasi_shim@0\.4\.2/);
  } finally {
    fixture.cleanup();
  }
});
