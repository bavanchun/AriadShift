import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import LicensesPage from "@/app/licenses/page";
import {
  pandocSourceInventory,
  renderPandocSourceTxt,
  verifyPandocSourceInventory,
} from "../pandoc-source-inventory.mjs";
import {
  generateWebRustNotices,
  parseClientBundleAnalysis,
  verifyCargoLockAgainstRustNotices,
} from "../scripts/sync-notices.mjs";

const webDir = path.resolve(import.meta.dirname, "..");
const rootDir = path.resolve(webDir, "../..");
const syncNoticesScript = path.join(webDir, "scripts/sync-notices.mjs");
const syncPandocScript = path.join(webDir, "scripts/sync-pandoc.mjs");

function noticeLinks(html: string): string[] {
  return [...html.matchAll(/href="((?:notices|wasm\/pandoc)\/[^"]+)"/g)].map(
    (match) => match[1] as string,
  );
}

function createCleanNoticeWorkspace(directory: string) {
  mkdirSync(path.join(directory, "apps/web/src/app/licenses"), { recursive: true });
  for (const name of ["NOTICE", "THIRD_PARTY_LICENSES", "crates", "node_modules"]) {
    symlinkSync(path.join(rootDir, name), path.join(directory, name));
  }
  for (const name of ["package.json", "node_modules", "src/app/licenses/page.tsx"]) {
    symlinkSync(path.join(webDir, name), path.join(directory, "apps/web", name));
  }
  for (const name of ["Cargo.lock", "pnpm-lock.yaml"]) {
    writeFileSync(path.join(directory, name), readFileSync(path.join(rootDir, name)));
  }
}

describe("web licence notices", () => {
  it("renders relative notice links, Pandoc 3.12 recipe/toolchain/inventory, MPL-2.0 crate sources and verbatim WASI shim licence on /licenses", () => {
    const html = renderToStaticMarkup(<LicensesPage />);
    const links = noticeLinks(html);

    expect(links).toEqual(
      expect.arrayContaining([
        "wasm/pandoc/COPYING.md",
        "wasm/pandoc/COPYRIGHT",
        "wasm/pandoc/SOURCE.txt",
        "notices/NOTICE.txt",
        "notices/THIRD_PARTY_LICENSES.txt",
        "notices/THIRD_PARTY_JS_LICENSES.txt",
      ]),
    );
    expect(html).not.toMatch(/href="\/(?:notices|wasm\/pandoc)\//);
    expect(html).not.toContain("https://github.com/bavanchun/AriadShift/blob/dev/NOTICE");

    // Pandoc 3.12 release tag, release asset, SHA-256 comparison, build recipe, freeze/lock status, and written offer.
    expect(html).toContain(pandocSourceInventory.releaseTagUrl);
    expect(html).toContain(pandocSourceInventory.archiveUrl);
    expect(html).toContain(pandocSourceInventory.archiveSha256);
    expect(html).toContain(pandocSourceInventory.upstreamArchiveDigest);
    expect(html).toContain(pandocSourceInventory.wasmSha256);
    expect(html).toContain(pandocSourceInventory.gitCommit.slice(0, 12));
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/.circleci/config.yml");
    expect(html).toContain(
      "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/.circleci/config.yml",
    );
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/Makefile");
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/wasm/Makefile");
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/flake.lock");
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/cabal.project");
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/stack.yaml");
    expect(html).toContain("cabal.project.freeze");
    expect(html).toContain("plan.json");
    expect(html).toContain("index-state");
    expect(html).toContain("not verified from public data");
    expect(html).toContain("The exact haskell-wasm/ghc fork commit used for the pandoc.wasm build");
    expect(html).toContain("at least three (3) years from the last distribution");
    expect(html).toContain(pandocSourceInventory.writtenOffer.contactUrl);
    expect(html).toContain(pandocSourceInventory.writtenOffer.repositoryUrl);
    expect(html).toContain(pandocSourceInventory.writtenOffer.securityAdvisoriesUrl);
    expect(html).toContain(
      "https://gitlab.haskell.org/haskell-wasm/wasi-libc/-/tree/ccb6b3d394fdfdfc0fdf855a4d2c99268842de30",
    );
    expect(html).toContain("https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release");
    expect(html).toContain(
      "https://hackage.haskell.org/package/pandoc-types-1.23.1.2/pandoc-types-1.23.1.2.tar.gz",
    );
    expect(html).toContain("https://hackage.haskell.org/package/typst-0.12/typst-0.12.tar.gz");

    // MPL-2.0 §3.2 source notice for unmodified MPL-2.0 crates in ariad-wasm.
    expect(html).toContain("MPL-2.0");
    expect(html).toContain("cssparser");
    expect(html).toContain("0.38.0");
    expect(html).toContain("https://crates.io/crates/cssparser/0.38.0");
    expect(html).toContain("https://github.com/servo/rust-cssparser");
    expect(html).toContain("dtoa-short");
    expect(html).toContain("0.3.5");
    expect(html).toContain("https://crates.io/crates/dtoa-short/0.3.5");
    expect(html).toContain("https://github.com/upsuper/dtoa-short");

    // Browser WASI shim attribution and verbatim upstream LICENSE-MIT without constructed copyright header.
    expect(html).toContain("@bjorn3/browser_wasi_shim");
    expect(html).toContain("0.4.2");
    expect(html).toContain("author: &quot;bjorn3&quot;");
    expect(html).toContain("https://github.com/bjorn3/browser_wasi_shim/tree/v0.4.2");
    expect(html).not.toMatch(/Copyright \(c\) bjorn3/i);
    expect(html).toContain("Permission is hereby granted, free of charge");
    expect(html).toContain("THE SOFTWARE IS PROVIDED &quot;AS IS&quot;");
  });

  it("verifies the Pandoc source inventory and fails when version, tag, asset hashes, build-definition permalinks, written offer or unit IDs diverge", () => {
    expect(() =>
      verifyPandocSourceInventory({
        version: pandocSourceInventory.version,
      }),
    ).not.toThrow();

    expect(() =>
      verifyPandocSourceInventory({
        version: "3.99",
      }),
    ).toThrow(/version/);

    expect(() =>
      verifyPandocSourceInventory({
        inventory: {
          ...pandocSourceInventory,
          gitTag: "3.11",
        },
      }),
    ).toThrow(/tag/);

    expect(() =>
      verifyPandocSourceInventory({
        inventory: {
          ...pandocSourceInventory,
          releaseTagUrl: "https://github.com/jgm/pandoc/releases/tag/3.11",
        },
      }),
    ).toThrow(/release tag URL/);

    expect(() =>
      verifyPandocSourceInventory({
        inventory: {
          ...pandocSourceInventory,
          archiveUrl: "https://github.com/jgm/pandoc/releases/download/3.11/pandoc-3.11.wasm.zip",
        },
      }),
    ).toThrow(/asset URL/);

    expect(() =>
      verifyPandocSourceInventory({
        archiveSha256: "0".repeat(64),
      }),
    ).toThrow(/archive SHA-256/);

    expect(() =>
      verifyPandocSourceInventory({
        wasmSha256: "f".repeat(64),
      }),
    ).toThrow(/pandoc\.wasm SHA-256/);

    expect(() =>
      verifyPandocSourceInventory({
        inventory: {
          ...pandocSourceInventory,
          recipe: {
            ...pandocSourceInventory.recipe,
            files: [
              {
                label: "Moving branch workflow",
                url: "https://github.com/jgm/pandoc/blob/main/.circleci/config.yml",
                commitUrl:
                  "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/.circleci/config.yml",
              },
            ],
          },
        },
      }),
    ).toThrow(/permalink/);

    expect(() =>
      verifyPandocSourceInventory({
        inventory: {
          ...pandocSourceInventory,
          writtenOffer: {
            ...pandocSourceInventory.writtenOffer,
            text: " Incomplete offer without validity period",
          },
        },
      }),
    ).toThrow(/written corresponding-source offer/);

    expect(() =>
      verifyPandocSourceInventory({
        inventory: {
          ...pandocSourceInventory,
          embeddedPackages: pandocSourceInventory.embeddedPackages.map((pkg) =>
            pkg.name === "aeson"
              ? { ...pkg, sourceUrl: "https://github.com/example/project/tree/feature-123" }
              : pkg,
          ),
        },
      }),
    ).toThrow(/Unstable or non-permalink/);

    expect(() =>
      verifyPandocSourceInventory({
        version: pandocSourceInventory.version,
        wasmBytes: Buffer.from("pandoc-3.12-inplace:Main\x00", "latin1"),
      }),
    ).toThrow(/unit IDs diverged/);

    const sourceTxt = renderPandocSourceTxt();
    expect(sourceTxt).toContain(pandocSourceInventory.releaseTagUrl);
    expect(sourceTxt).toContain(pandocSourceInventory.archiveUrl);
    expect(sourceTxt).toContain(pandocSourceInventory.archiveSha256);
    expect(sourceTxt).toContain(pandocSourceInventory.upstreamArchiveDigest);
    expect(sourceTxt).toContain(pandocSourceInventory.wasmSha256);
    expect(sourceTxt).toContain("Written Corresponding-Source Offer (Three Years)");
    expect(sourceTxt).toContain(pandocSourceInventory.writtenOffer.text);
    expect(sourceTxt).toContain(
      "Build Recipe and Upstream Build Definitions (Tag 3.12 Permalinks)",
    );
    expect(sourceTxt).toContain("Upstream Freeze, Lock, and Plan Files at Tag 3.12");
    expect(sourceTxt).toContain("cabal.project.freeze");
    expect(sourceTxt).toContain("plan.json");
    expect(sourceTxt).toContain("index-state");
    expect(sourceTxt).toContain("not verified from public data");
    expect(sourceTxt).toContain(
      "The exact haskell-wasm/ghc fork commit used for the pandoc.wasm build",
    );
    expect(sourceTxt).toContain("Compiler, WASI Libc, and Toolchain Sources");
    expect(sourceTxt).toContain("Linked Haskell Packages Verified in pandoc.wasm");
    expect(sourceTxt).toContain(
      "Additional Cabal Dependencies (No Upstream wasm32-wasi Freeze File)",
    );
  });

  it("wires sync-notices and post-generation --check into dev, build, export and justfile", () => {
    const pkg = JSON.parse(readFileSync(path.join(webDir, "package.json"), "utf8")) as {
      scripts?: Record<string, string>;
    };
    for (const script of ["dev", "build", "export"]) {
      expect(pkg.scripts?.[script], `${script} must run sync-notices.mjs`).toContain(
        "node scripts/sync-notices.mjs",
      );
    }
    for (const script of ["build", "export"]) {
      expect(
        pkg.scripts?.[script],
        `${script} must run sync-notices.mjs --check after build/export`,
      ).toContain("node scripts/sync-notices.mjs --check");
    }
    const exportScript = readFileSync(path.join(webDir, "scripts/export.mjs"), "utf8");
    expect(exportScript).toContain("checkNotices()");

    const justfile = readFileSync(path.join(rootDir, "justfile"), "utf8");
    expect(justfile).toMatch(/^web-licenses:\s*\n\s+node apps\/web\/scripts\/sync-notices\.mjs/m);
    expect(justfile).toMatch(
      /^web-licenses-check:\s*\n\s+node apps\/web\/scripts\/sync-notices\.mjs --check/m,
    );
  });

  it("generates web notices, resolves relative links under root and path prefixes, and covers production bundle dependencies", {
    timeout: 20_000,
  }, () => {
    const syncResult = spawnSync(process.execPath, [syncNoticesScript], {
      cwd: webDir,
      encoding: "utf8",
    });
    expect(syncResult.status, syncResult.stderr).toBe(0);

    const pandocResult = spawnSync(process.execPath, [syncPandocScript], {
      cwd: webDir,
      encoding: "utf8",
    });
    expect(pandocResult.status, pandocResult.stderr).toBe(0);

    const html = renderToStaticMarkup(<LicensesPage />);
    const links = noticeLinks(html);
    expect(links.length).toBeGreaterThanOrEqual(6);

    for (const href of links) {
      const rootResolved = new URL(href, "https://example.test/licenses").pathname;
      expect(rootResolved).toBe(`/${href}`);
      const prefixResolved = new URL(href, "https://example.test/AriadShift/licenses").pathname;
      expect(prefixResolved).toBe(`/AriadShift/${href}`);

      const publicFile = path.join(webDir, "public", href);
      expect(existsSync(publicFile), `${href} must exist in public/`).toBe(true);
      expect(readFileSync(publicFile, "utf8").length).toBeGreaterThan(0);
    }

    const noticeText = readFileSync(path.join(webDir, "public/notices/NOTICE.txt"), "utf8");
    expect(noticeText).toContain("Apache License, Version 2.0");
    expect(noticeText).toContain("SIL OPEN FONT LICENSE Version 1.1");
    expect(noticeText).toContain(pandocSourceInventory.releaseTagUrl);
    expect(noticeText).toContain(pandocSourceInventory.archiveUrl);
    expect(noticeText).toContain(pandocSourceInventory.archiveSha256);
    expect(noticeText).toContain(pandocSourceInventory.wasmSha256);
    expect(noticeText).toContain("not verified from public data");
    expect(noticeText).toContain(pandocSourceInventory.writtenOffer.text);

    const pandocSourceText = readFileSync(
      path.join(webDir, "public/wasm/pandoc/SOURCE.txt"),
      "utf8",
    );
    expect(pandocSourceText).toContain(pandocSourceInventory.releaseTagUrl);
    expect(pandocSourceText).toContain(pandocSourceInventory.archiveUrl);
    expect(pandocSourceText).toContain(pandocSourceInventory.archiveSha256);
    expect(pandocSourceText).toContain(pandocSourceInventory.wasmSha256);
    expect(pandocSourceText).toContain("not verified from public data");
    expect(pandocSourceText).toContain(pandocSourceInventory.writtenOffer.text);

    const rustNotices = readFileSync(
      path.join(webDir, "public/notices/THIRD_PARTY_LICENSES.txt"),
      "utf8",
    );
    expect(rustNotices).toContain("MPL-2.0");

    const jsNotices = readFileSync(
      path.join(webDir, "public/notices/THIRD_PARTY_JS_LICENSES.txt"),
      "utf8",
    );
    expect(jsNotices).toContain("@bjorn3/browser_wasi_shim@0.4.2");
    expect(jsNotices).toContain("@fontsource-variable/plus-jakarta-sans@5.3.0");
    expect(jsNotices).toContain("@opentelemetry/api@1.10.0");
    expect(jsNotices).toContain("pg@8.23.1");
    expect(jsNotices).toContain("sharp@0.35.5");
    expect(jsNotices).toContain("next@16.4.2");
    expect(jsNotices).toContain("react@19.3.1");
  });

  it("rejects an omitted transitive Rust dependency and an empty bundle graph", () => {
    const notices = generateWebRustNotices(rootDir);
    expect(notices).toContain("  - dtoa-short 0.3.5");
    expect(() =>
      verifyCargoLockAgainstRustNotices(
        rootDir,
        notices.replace(/^ {2}- dtoa-short 0\.3\.5.*\n/m, ""),
      ),
    ).toThrow(/dtoa-short 0\.3\.5/);
    expect(() =>
      verifyCargoLockAgainstRustNotices(rootDir, `${notices}\n  - unrelated-crate 1.0.0\n`),
    ).toThrow(/unrelated-crate 1\.0\.0/);
    expect(() =>
      parseClientBundleAnalysis('{"type":"meta","schema_version":1,"selected_routes":1}\n'),
    ).toThrow(/Empty or unrecognized/);
    const evidence = [
      { type: "meta", schema_version: 1, selected_routes: 1 },
      {
        type: "part",
        filename: "[client-fs]/_next/static/chunks/test.js",
        source_path:
          "[project]/node_modules/.pnpm/@bjorn3+browser_wasi_shim@0.4.2/node_modules/@bjorn3/browser_wasi_shim/dist/index.js",
      },
    ]
      .map((record) => JSON.stringify(record))
      .join("\n");
    expect(parseClientBundleAnalysis(evidence)).toEqual(["@bjorn3/browser_wasi_shim@0.4.2"]);
    expect(() => parseClientBundleAnalysis("not JSON")).toThrow(/invalid JSON Lines/);
    expect(() =>
      parseClientBundleAnalysis(evidence.replace('"schema_version":1', '"schema_version":2')),
    ).toThrow(/Empty or unrecognized/);
    expect(() => parseClientBundleAnalysis(evidence.replace(".pnpm/", "unknown-layout/"))).toThrow(
      /Unrecognized bundled package path/,
    );
    expect(() => parseClientBundleAnalysis(evidence.replace("@0.4.2/", "@unknown/"))).toThrow(
      /Unrecognized package version/,
    );
  });

  it("checks the default production graph without generated notices or build analysis", {
    timeout: 30_000,
  }, () => {
    const fixtureDir = mkdtempSync(path.join(tmpdir(), "ariad-clean-notice-graph-"));
    try {
      createCleanNoticeWorkspace(fixtureDir);
      const env = { ...process.env, ARIADSHIFT_ROOT_DIR: fixtureDir };
      const generatedPaths = [".next", "out", "public/notices", "public/wasm/pandoc"].map((name) =>
        path.join(fixtureDir, "apps/web", name),
      );
      for (const target of generatedPaths) expect(existsSync(target), target).toBe(false);

      // The root override makes this the CLI's default notices directory, without
      // --out-dir or --root-dir bypassing the normal production validation path.
      const graph = spawnSync(process.execPath, [syncNoticesScript, "--check-graph"], {
        cwd: path.join(fixtureDir, "apps/web"),
        env,
        encoding: "utf8",
      });
      expect(graph.status, graph.stderr).toBe(0);
      for (const target of generatedPaths) expect(existsSync(target), target).toBe(false);

      // Full post-build validation still requires actual current-build evidence.
      const generate = spawnSync(process.execPath, [syncNoticesScript], {
        cwd: path.join(fixtureDir, "apps/web"),
        env,
        encoding: "utf8",
      });
      expect(generate.status, generate.stderr).toBe(0);
      const full = spawnSync(process.execPath, [syncNoticesScript, "--check"], {
        cwd: path.join(fixtureDir, "apps/web"),
        env,
        encoding: "utf8",
      });
      expect(full.status).not.toBe(0);
      expect(full.stderr).toContain("Cannot read Next.js bundle analysis");
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true });
    }
  });

  it("fails the real sync-notices.mjs script on stale notice files or stale lockfile fixtures", {
    timeout: 90_000,
  }, () => {
    const fixtureDir = mkdtempSync(path.join(tmpdir(), "ariad-web-notices-"));
    try {
      const syncResult = spawnSync(process.execPath, [syncNoticesScript, "--out-dir", fixtureDir], {
        cwd: webDir,
        encoding: "utf8",
      });
      expect(syncResult.status, syncResult.stderr).toBe(0);

      const cleanCheck = spawnSync(
        process.execPath,
        [syncNoticesScript, "--check", "--out-dir", fixtureDir],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(cleanCheck.status, cleanCheck.stderr).toBe(0);

      for (const fileName of [
        "THIRD_PARTY_JS_LICENSES.txt",
        "NOTICE.txt",
        "THIRD_PARTY_LICENSES.txt",
      ]) {
        const target = path.join(fixtureDir, fileName);
        const original = readFileSync(target, "utf8");
        writeFileSync(target, `${original}\nstale fixture line\n`, "utf8");

        const staleCheck = spawnSync(
          process.execPath,
          [syncNoticesScript, "--check", "--out-dir", fixtureDir],
          { cwd: webDir, encoding: "utf8" },
        );
        expect(staleCheck.status).not.toBe(0);
        expect(staleCheck.stderr).toContain(fileName);

        writeFileSync(target, original, "utf8");
      }

      unlinkSync(path.join(fixtureDir, "THIRD_PARTY_JS_LICENSES.txt"));
      const missingCheck = spawnSync(
        process.execPath,
        [syncNoticesScript, "--check", "--out-dir", fixtureDir],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(missingCheck.status).not.toBe(0);
      expect(missingCheck.stderr).toContain("THIRD_PARTY_JS_LICENSES.txt");

      // Create a workspace fixture with a deliberately stale pnpm-lock.yaml and Cargo.lock
      // and verify that both the default generation entry and --check fail rather than silently overwriting.
      const rootFixture = path.join(fixtureDir, "root");
      createCleanNoticeWorkspace(rootFixture);

      const realCargoLock = readFileSync(path.join(rootDir, "Cargo.lock"), "utf8");
      const realPnpmLock = readFileSync(path.join(rootDir, "pnpm-lock.yaml"), "utf8");
      const cleanFixture = spawnSync(
        process.execPath,
        [syncNoticesScript, "--root-dir", rootFixture],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(cleanFixture.status, cleanFixture.stderr).toBe(0);
      expect(existsSync(path.join(rootFixture, "apps/web/public/notices/NOTICE.txt"))).toBe(true);

      // 1. Stale pnpm-lock.yaml (missing clsx@2.1.1 from snapshots -> drifts from installed node_modules)
      writeFileSync(
        path.join(rootFixture, "pnpm-lock.yaml"),
        realPnpmLock.replace("  clsx@2.1.1: {}", "  clsx@9.9.9: {}"),
        "utf8",
      );
      const stalePnpmSync = spawnSync(
        process.execPath,
        [syncNoticesScript, "--root-dir", rootFixture],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(stalePnpmSync.status).not.toBe(0);
      expect(stalePnpmSync.stderr).toMatch(/pnpm-lock\.yaml/);

      for (const script of ["build", "export"]) {
        const result = spawnSync("pnpm", ["run", script], {
          cwd: webDir,
          encoding: "utf8",
          env: { ...process.env, ARIADSHIFT_ROOT_DIR: rootFixture },
        });
        expect(result.status, `${script}: ${result.stderr}`).not.toBe(0);
        expect(result.stderr).toMatch(/JavaScript notices cannot be checked: pnpm-lock\.yaml/);
      }

      // 2. Stale Cargo.lock (version bump in Cargo.lock not reflected in THIRD_PARTY_LICENSES)
      writeFileSync(path.join(rootFixture, "pnpm-lock.yaml"), realPnpmLock, "utf8");
      writeFileSync(
        path.join(rootFixture, "Cargo.lock"),
        realCargoLock.replace(
          'name = "dtoa-short"\nversion = "0.3.5"',
          'name = "dtoa-short"\nversion = "0.9.9"',
        ),
        "utf8",
      );
      const staleCargoSync = spawnSync(
        process.execPath,
        [syncNoticesScript, "--root-dir", rootFixture],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(staleCargoSync.status).not.toBe(0);
      expect(staleCargoSync.stderr).toMatch(/Cargo\.lock/);

      // 3. Stale Pandoc engine-assets.mjs version or SHA-256 in fixture
      writeFileSync(path.join(rootFixture, "Cargo.lock"), realCargoLock, "utf8");
      const realEngineAssets = readFileSync(path.join(webDir, "engine-assets.mjs"), "utf8");
      writeFileSync(
        path.join(rootFixture, "apps/web/engine-assets.mjs"),
        realEngineAssets.replace(pandocSourceInventory.wasmSha256, "0".repeat(64)),
        "utf8",
      );
      const stalePandocHashSync = spawnSync(
        process.execPath,
        [syncNoticesScript, "--check", "--root-dir", rootFixture],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(stalePandocHashSync.status).not.toBe(0);
      expect(stalePandocHashSync.stderr).toMatch(/pandoc\.wasm SHA-256/);

      // 4. Moving-branch URL or missing written-offer reference in page.tsx fixture
      writeFileSync(path.join(rootFixture, "apps/web/engine-assets.mjs"), realEngineAssets, "utf8");
      const realPage = readFileSync(path.join(webDir, "src/app/licenses/page.tsx"), "utf8");
      unlinkSync(path.join(rootFixture, "apps/web/src/app/licenses/page.tsx"));
      writeFileSync(
        path.join(rootFixture, "apps/web/src/app/licenses/page.tsx"),
        realPage.replace(
          "pandocSourceInventory.writtenOffer.text",
          '"https://github.com/jgm/pandoc/blob/main/.circleci/config.yml"',
        ),
        "utf8",
      );
      const stalePageSync = spawnSync(
        process.execPath,
        [syncNoticesScript, "--check", "--root-dir", rootFixture],
        { cwd: webDir, encoding: "utf8" },
      );
      expect(stalePageSync.status).not.toBe(0);
      expect(stalePageSync.stderr).toMatch(/moving branch|writtenOffer/);
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true });
    }
  });
});
