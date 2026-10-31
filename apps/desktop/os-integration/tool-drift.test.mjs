import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, sep } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { loadContext, render, stale } from "./generate.mjs";

test("the actual desktop tool command rejects stale OS integration output", () => {
  assert.deepEqual(stale(render(loadContext())), []);
  const scratchRoot = mkdtempSync(join(tmpdir(), "ariadshift-tool-drift-"));
  const scratch = join(scratchRoot, "apps/desktop");
  try {
    mkdirSync(scratch, { recursive: true });
    const desktop = fileURLToPath(new URL("../", import.meta.url));
    for (const path of ["package.json", "scripts", "os-integration", "src-tauri"]) {
      cpSync(join(desktop, path), join(scratch, path), {
        recursive: true,
        // The nested tool command must not recursively run this scratch-copy check.
        filter: (source) => !source.endsWith("tool-drift.test.mjs"),
      });
    }
    const context = loadContext();
    for (const { source } of context.resourceFiles) {
      const target = join(scratch, "src-tauri/windows", source);
      assert.ok(target.startsWith(`${scratchRoot}${sep}`));
      mkdirSync(dirname(target), { recursive: true });
      cpSync(join(desktop, "src-tauri/windows", source), target);
    }
    context.formats.find((format) => format.id === "markdown").extensions.push("drift-probe");
    writeFileSync(join(scratch, "os-integration/formats.json"), JSON.stringify(context));
    // These Node-only tools need no dependency installation in the disposable copy.
    writeFileSync(join(scratch, "pnpm-workspace.yaml"), "verifyDepsBeforeRun: false\n");
    // The subprocess needs tool lookup and user paths, without Node's test-child protocol.
    const environment = Object.fromEntries(
      [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "APPDATA",
        "LOCALAPPDATA",
        "COMSPEC",
        "PATHEXT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "MISE_NODE_VERSION",
        "MISE_PNPM_VERSION",
      ]
        .filter((name) => process.env[name] !== undefined)
        .map((name) => [name, process.env[name]]),
    );
    const child = spawnSync("pnpm run test:tools", {
      cwd: scratch,
      env: environment,
      shell: true,
      encoding: "utf8",
      timeout: 60_000,
    });
    assert.ifError(child.error);
    const output = child.stdout + child.stderr;
    assert.equal(child.status, 1, output);
    assert.match(output, /(?:not ok \d+ - |✖ )committed OS integration files match formats.json/);
  } finally {
    rmSync(scratchRoot, { recursive: true, force: true });
  }
});
