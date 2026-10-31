import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { label, loadContext, render, stale, verbKey } from "./generate.mjs";

const context = loadContext();
const files = render(context);
const read = (path) => JSON.parse(readFileSync(new URL(`../src-tauri/${path}`, import.meta.url)));

test("committed OS integration files match formats.json", () => {
  assert.deepEqual(stale(files), []);
});

test("every supported extension gets an Explorer verb that is installed and removed", () => {
  const nsis = files["src-tauri/windows/os-integration.nsh"];
  const wix = files["src-tauri/windows/os-integration.wxs"];
  for (const extension of context.formats.flatMap((format) => format.extensions)) {
    const key = `SystemFileAssociations\\.${extension}\\shell\\${verbKey}`;
    for (const command of ["WriteRegStr", "DeleteRegKey"]) {
      const line = `${command} SHCTX "Software\\Classes\\${key}`;
      assert.ok(nsis.includes(line), line);
    }
    assert.ok(wix.includes(`Key="Software\\Classes\\${key}"`), extension);
  }
  assert.ok(!nsis.includes("json") && !wix.includes(".json"), "JSON is not registered");
});

test("Linux entries pass files and cover every media type", () => {
  const desktop = files["os-integration/linux/main.desktop.hbs"];
  const dolphin = files["os-integration/linux/dolphin/ariadshift-convert.desktop"];
  assert.match(desktop, /^Exec=\{\{exec\}\} %U$/m);
  assert.match(dolphin, new RegExp(`^Exec=${context.binary} -- %F$`, "m"));
  for (const { media_type: type } of context.formats) {
    assert.ok(desktop.includes(`${type};`) && dolphin.includes(`${type};`), type);
  }
  const nautilus = files["os-integration/linux/nautilus/convert-with-ariadshift"];
  assert.match(nautilus, new RegExp(`^exec ${context.binary} -- "\\$@"$`, "m"));
});

test("bundle configs ship the generated files and hooks", () => {
  const linux = read("tauri.linux.conf.json").bundle.linux;
  for (const target of [linux.deb, linux.rpm]) {
    assert.equal(target.desktopTemplate, "../os-integration/linux/main.desktop.hbs");
    for (const source of Object.values(target.files)) {
      assert.ok(files[source.replace("../", "")] !== undefined, source);
    }
  }
  const windows = read("tauri.windows.conf.json").bundle.windows;
  assert.equal(windows.nsis.installMode, "currentUser");
  assert.equal(windows.nsis.installerHooks, "windows/os-integration.nsh");
  assert.deepEqual(windows.wix.fragmentPaths, ["windows/os-integration.wxs"]);
  assert.ok(files["src-tauri/windows/os-integration.wxs"].includes('Id="ExplorerConvertVerbs"'));
  assert.deepEqual(windows.wix.componentRefs, ["ExplorerConvertVerbs"]);
  const mac = read("tauri.macos.conf.json").bundle.macOS;
  const workflow = Object.values(mac.files).filter((source) => source.includes(".workflow/"));
  assert.equal(workflow.length, 2);
  for (const source of workflow) assert.ok(files[source.replace("../", "")] !== undefined, source);
});

test("the Quick Action opens the app by bundle identifier and Finder sees the label", () => {
  const document = files[`os-integration/macos/${label}.workflow/Contents/document.wflow`];
  assert.ok(document.includes(`/usr/bin/open -b ${context.identifier} "$@"`));
  const info = files[`os-integration/macos/${label}.workflow/Contents/Info.plist`];
  assert.ok(info.includes(`<string>${label}</string>`));
  assert.ok(files["src-tauri/Info.plist"].includes("<string>Alternate</string>"));
});
