import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import LicensesPage from "@/app/licenses/page";

const webDir = path.resolve(import.meta.dirname, "..");
const rootDir = path.resolve(webDir, "../..");
const syncNoticesScript = path.join(webDir, "scripts/sync-notices.mjs");
const syncPandocScript = path.join(webDir, "scripts/sync-pandoc.mjs");

function localLinks(html: string): string[] {
  return [...html.matchAll(/href="(\/[^"]+)"/g)].map((match) => match[1] as string);
}

describe("web licence notices", () => {
  it("links local notices, Pandoc SOURCE.txt, MPL-2.0 crate sources and WASI shim licence text on /licenses", () => {
    const html = renderToStaticMarkup(<LicensesPage />);
    const links = localLinks(html);

    expect(links).toEqual(
      expect.arrayContaining([
        "/wasm/pandoc/COPYING.md",
        "/wasm/pandoc/COPYRIGHT",
        "/wasm/pandoc/SOURCE.txt",
        "/notices/NOTICE.txt",
        "/notices/THIRD_PARTY_LICENSES.txt",
        "/notices/THIRD_PARTY_JS_LICENSES.txt",
      ]),
    );
    expect(html).not.toContain("https://github.com/bavanchun/AriadShift/blob/dev/NOTICE");

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

    // Browser WASI shim copyright and permission text.
    expect(html).toContain("@bjorn3/browser_wasi_shim");
    expect(html).toContain("0.4.2");
    expect(html).toContain("Copyright (c) bjorn3 and contributors");
    expect(html).toContain("Permission is hereby granted, free of charge");
    expect(html).toContain("THE SOFTWARE IS PROVIDED &quot;AS IS&quot;");
  });

  it("wires sync-notices into dev, build, export and justfile", () => {
    const pkg = JSON.parse(readFileSync(path.join(webDir, "package.json"), "utf8")) as {
      scripts?: Record<string, string>;
    };
    for (const script of ["dev", "build", "export"]) {
      expect(pkg.scripts?.[script], `${script} must run sync-notices.mjs`).toContain(
        "node scripts/sync-notices.mjs",
      );
    }
    const justfile = readFileSync(path.join(rootDir, "justfile"), "utf8");
    expect(justfile).toMatch(/^web-licenses:\s*\n\s+node apps\/web\/scripts\/sync-notices\.mjs/m);
    expect(justfile).toMatch(
      /^web-licenses-check:\s*\n\s+node apps\/web\/scripts\/sync-notices\.mjs --check/m,
    );
  });

  it("generates web notices and resolves every local link on /licenses", () => {
    expect(existsSync(syncNoticesScript)).toBe(true);
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
    const links = localLinks(html);
    expect(links.length).toBeGreaterThanOrEqual(6);

    for (const href of links) {
      const publicFile = path.join(webDir, "public", href);
      expect(existsSync(publicFile), `${href} must exist in public/`).toBe(true);
      expect(readFileSync(publicFile, "utf8").length).toBeGreaterThan(0);
    }

    const nextStartHtmlPath = path.join(webDir, ".next/server/app/licenses.html");
    if (existsSync(nextStartHtmlPath)) {
      const nextStartLinks = localLinks(readFileSync(nextStartHtmlPath, "utf8")).filter(
        (href) => href.startsWith("/notices/") || href.startsWith("/wasm/pandoc/"),
      );
      expect(nextStartLinks.length).toBeGreaterThanOrEqual(6);
      for (const href of nextStartLinks) {
        const publicFile = path.join(webDir, "public", href);
        expect(existsSync(publicFile), `${href} must resolve in next start public/`).toBe(true);
        expect(readFileSync(publicFile, "utf8").length).toBeGreaterThan(0);
      }
    }

    const exportHtmlPath = path.join(webDir, "out/licenses.html");
    if (existsSync(exportHtmlPath)) {
      const exportLinks = localLinks(readFileSync(exportHtmlPath, "utf8")).filter(
        (href) => href.startsWith("/notices/") || href.startsWith("/wasm/pandoc/"),
      );
      expect(exportLinks.length).toBeGreaterThanOrEqual(6);
      for (const href of exportLinks) {
        const exportFile = path.join(webDir, "out", href);
        expect(existsSync(exportFile), `${href} must resolve in export out/`).toBe(true);
        expect(readFileSync(exportFile, "utf8").length).toBeGreaterThan(0);
      }
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
    expect(jsNotices).toContain("next@16.4.2");
    expect(jsNotices).toContain("react@19.3.1");
  });

  it("fails the staleness check on deliberately stale or missing notice fixtures", () => {
    expect(existsSync(syncNoticesScript)).toBe(true);
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
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true });
    }
  });
});
