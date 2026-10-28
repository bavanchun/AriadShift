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
import { collectBundledNodeModulesPackages } from "../scripts/sync-notices.mjs";

const webDir = path.resolve(import.meta.dirname, "..");
const rootDir = path.resolve(webDir, "../..");
const syncNoticesScript = path.join(webDir, "scripts/sync-notices.mjs");
const syncPandocScript = path.join(webDir, "scripts/sync-pandoc.mjs");

function noticeLinks(html: string): string[] {
  return [...html.matchAll(/href="((?:notices|wasm\/pandoc)\/[^"]+)"/g)].map(
    (match) => match[1] as string,
  );
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

    // Pandoc 3.12 recipe, toolchain, and linked Hackage package inventory.
    expect(html).toContain(pandocSourceInventory.gitCommit.slice(0, 12));
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/.circleci/config.yml");
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/flake.lock");
    expect(html).toContain("https://github.com/jgm/pandoc/blob/3.12/cabal.project");
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

  it("verifies the Pandoc source inventory and fails when version, URLs or unit IDs diverge", () => {
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
        version: pandocSourceInventory.version,
        wasmBytes: Buffer.from("pandoc-3.12-inplace:Main\x00", "latin1"),
      }),
    ).toThrow(/unit IDs diverged/);

    const sourceTxt = renderPandocSourceTxt();
    expect(sourceTxt).toContain("Build Recipe (Upstream Release Binary)");
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

    const bundledPackages = collectBundledNodeModulesPackages(webDir);
    for (const pkgName of bundledPackages) {
      expect(jsNotices, `bundled package ${pkgName} must be in JS notices`).toMatch(
        new RegExp(`^${pkgName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}@\\d+`, "m"),
      );
    }
  });

  it("fails the real sync-notices.mjs script on stale notice files or stale lockfile fixtures", {
    timeout: 20_000,
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
      mkdirSync(path.join(rootFixture, "apps/web/src/app/licenses"), { recursive: true });
      mkdirSync(path.join(rootFixture, "apps/web/public/notices"), { recursive: true });
      symlinkSync(path.join(rootDir, "NOTICE"), path.join(rootFixture, "NOTICE"));
      symlinkSync(
        path.join(rootDir, "THIRD_PARTY_LICENSES"),
        path.join(rootFixture, "THIRD_PARTY_LICENSES"),
      );
      symlinkSync(path.join(rootDir, "crates"), path.join(rootFixture, "crates"));
      symlinkSync(path.join(rootDir, "node_modules"), path.join(rootFixture, "node_modules"));
      symlinkSync(
        path.join(webDir, "package.json"),
        path.join(rootFixture, "apps/web/package.json"),
      );
      symlinkSync(
        path.join(webDir, "node_modules"),
        path.join(rootFixture, "apps/web/node_modules"),
      );
      symlinkSync(
        path.join(webDir, "src/app/licenses/page.tsx"),
        path.join(rootFixture, "apps/web/src/app/licenses/page.tsx"),
      );

      const realCargoLock = readFileSync(path.join(rootDir, "Cargo.lock"), "utf8");
      const realPnpmLock = readFileSync(path.join(rootDir, "pnpm-lock.yaml"), "utf8");

      // 1. Stale pnpm-lock.yaml (missing clsx@2.1.1 from snapshots -> drifts from installed node_modules)
      writeFileSync(path.join(rootFixture, "Cargo.lock"), realCargoLock, "utf8");
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
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true });
    }
  });
});
