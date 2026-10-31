import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  checkWindowsOutput,
  loadContext,
  mergeConfig,
  render,
  windowsTauriArgs,
} from "./generate.mjs";

const windows = new URL("../src-tauri/", import.meta.url);
const config = JSON.parse(readFileSync(new URL("tauri.windows.conf.json", windows))).bundle.windows;
const files = render(loadContext());

test("the MSI package installs for the current user without elevation", () => {
  assert.equal(config.wix.template, "windows/per-user.wxs");
  const template = readFileSync(new URL(config.wix.template, windows), "utf8");
  const cliVersion = JSON.parse(readFileSync(new URL("../package.json", windows))).devDependencies[
    "@tauri-apps/cli"
  ];
  assert.ok(template.includes(`@tauri-apps/cli ${cliVersion}`), "review template on CLI upgrades");
  assert.match(template, /<Package\b[^>]*InstallScope="perUser"[^>]*>/s);
  assert.match(template, /<Package\b[^>]*InstallPrivileges="limited"[^>]*>/s);
  assert.match(template, /<Directory Id="LocalAppDataFolder">\s*<Directory Id="INSTALLDIR"/s);
  assert.doesNotMatch(template, /InstallScope="perMachine"|<Property Id="ALLUSERS"/);
  assert.doesNotMatch(template, /<RegistryKey\b[^>]*Root="HKLM"/s);
  assert.match(template, /<RegistryKey Root="HKCU" Key="Software\\Classes\\\\\{\{protocol\}\}"/);
  assert.match(
    template,
    /<CustomAction Id="UpdateWebView2ViaEdgeUpdate"[^>]*Impersonate="yes"[^>]*needsadmin=false/,
  );
  assert.notEqual(config.wix.enableElevatedUpdateTask, true);
});

test("per-user MSI keeps file, fragment, upgrade and uninstall integration", () => {
  assert.equal(config.wix.template, "windows/per-user.wxs");
  const template = readFileSync(new URL(config.wix.template, windows), "utf8");
  for (const variable of [
    "upgrade_code",
    "main_binary_path",
    "component_refs",
    "component_group_refs",
    "feature_refs",
    "feature_group_refs",
    "merge_refs",
  ]) {
    assert.ok(template.includes(variable), variable);
  }
  assert.match(template, /<File Id="Path" Source="\{\{main_binary_path\}\}"/);
  assert.match(template, /<MajorUpgrade Schedule="afterInstallInitialize"/);
  assert.match(template, /DowngradeErrorMessage="!\(loc.DowngradeErrorMessage\)"/);
  assert.match(
    template,
    /<RemoveShortcuts>Installed AND NOT UPGRADINGPRODUCTCODE<\/RemoveShortcuts>/,
  );
  assert.match(template, /<RemoveFolder Id="INSTALLDIR"\s+On="uninstall"/);
  assert.deepEqual(config.wix.componentRefs, ["ExplorerConvertVerbs"]);
  assert.deepEqual(config.wix.fragmentPaths, ["windows/os-integration.wxs"]);
  assert.deepEqual(config.wix.componentGroupRefs, ["PerUserBundleResources"]);
  const fragment = files["src-tauri/windows/os-integration.wxs"];
  assert.match(fragment, /<RegistryValue Root="HKCU"[^>]*KeyPath="yes"/);
  assert.doesNotMatch(fragment, /Root="HKLM"/);
  const keys = [...fragment.matchAll(/<RegistryKey Root="HKCU"[^>]*>/g)];
  assert.equal(keys.length, loadContext().formats.flatMap((format) => format.extensions).length);
  for (const [key] of keys) assert.ok(key.includes('ForceDeleteOnUninstall="yes"'), key);
});

test("profile files and generated resources use HKCU keypaths", () => {
  const template = readFileSync(new URL(config.wix.template, windows), "utf8");
  const main = /<Component Id="Path"[\s\S]*?<\/Component>/.exec(template)?.[0];
  assert.ok(main);
  assert.match(main, /Guid="[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}"/);
  assert.match(main, /<RegistryValue Root="HKCU"[^>]*KeyPath="yes"/);
  assert.match(main, /<File Id="Path"[^>]*KeyPath="no"/);
  const binaries = /\{\{#each binaries[\s\S]*?\{\{\/each/.exec(template)?.[0];
  assert.ok(binaries);
  assert.match(binaries, /Guid="\{\{\s*bin\.guid\s*\}\}"/);
  assert.match(binaries, /<RegistryValue Root="HKCU"[^>]*KeyPath="yes"/);
  assert.match(binaries, /<File Id="Bin_[^>]*KeyPath="no"/);
  assert.doesNotMatch(template, /\{\{resources\}\}|\{\{#each resource_file_ids/);
  const fragment = files["src-tauri/windows/os-integration.wxs"];
  assert.match(fragment, /<ComponentGroup Id="PerUserBundleResources">/);
  const resources = /<Component Id="BundleResources"[\s\S]*?<\/Component>/.exec(fragment)?.[0];
  assert.ok(resources);
  assert.match(resources, /<RegistryValue Root="HKCU"[^>]*KeyPath="yes"/);
  for (const { source, name } of loadContext().resourceFiles) {
    assert.ok(resources.includes(`Source="$(sys.SOURCEFILEDIR)${source.replaceAll("/", "\\")}"`));
    assert.ok(resources.includes(`Name="${name}" KeyPath="no"`));
  }
});

test("Windows packaging binds canonical output and rejects hidden resource inputs", () => {
  const host = "x86_64-pc-windows-msvc";
  assert.deepEqual(windowsTauriArgs(["build"], host, {}), {
    args: ["build"],
    windows: true,
    output: "native",
  });
  for (const args of [
    ["-v", "build"],
    ["--verbose", "bundle"],
  ]) {
    assert.equal(windowsTauriArgs(args, host, {}).output, "native");
  }
  for (const args of [
    ["build", "--target", host],
    ["bundle", `-t${host}`],
  ]) {
    assert.deepEqual(windowsTauriArgs(args, host, {}, true), {
      args,
      windows: true,
      output: "target",
    });
  }
  assert.throws(() => windowsTauriArgs(["build"], host, {}, true));
  assert.deepEqual(windowsTauriArgs(["dev"], host, {}).args, ["dev"]);
  assert.deepEqual(windowsTauriArgs(["build"], "aarch64-apple-darwin", {}).args, ["build"]);
  for (const options of [
    ["--profile", "custom"],
    ["--target-dir=elsewhere"],
    ["--", "--config", "build.target-dir=elsewhere"],
    ["--", "--target", "aarch64-pc-windows-msvc"],
    ["--debug", "--", "--release"],
    ["--debug", "--", "-rv"],
    ["--debug", "--", "-vrj4"],
    ["--", "-m", "elsewhere/Cargo.toml"],
    ["--", "-vmelsewhere/Cargo.toml"],
  ])
    assert.throws(() => windowsTauriArgs(["build", ...options], host, {}));
  assert.doesNotThrow(() => windowsTauriArgs(["build", "--", "-Fserde"], host, {}));
  for (const environment of [{ TAURI_CONFIG: "{}" }, { CARGO_BUILD_TARGET: host }]) {
    assert.throws(() => windowsTauriArgs(["bundle"], host, environment));
  }
  assert.throws(() => loadContext({ bundle: { windows: { bundleVcRuntime: true } } }));
  assert.throws(() => loadContext({ bundle: { resources: ["assets/**"] } }));
  assert.deepEqual(mergeConfig({ a: { b: 1, c: [1] } }, { a: { b: null, c: [2] } }), {
    a: { c: [2] },
  });
  const directory = mkdtempSync(join(tmpdir(), "ariadshift-msi-output-"));
  try {
    assert.throws(() => checkWindowsOutput(directory, "ariad-desktop"));
    writeFileSync(join(directory, "ariad-desktop.exe"), "");
    assert.doesNotThrow(() => checkWindowsOutput(directory, "ariad-desktop"));
    writeFileSync(join(directory, "WebView2Loader.DLL"), "");
    assert.throws(() => checkWindowsOutput(directory, "ariad-desktop"), /output DLLs/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
  const effective = JSON.parse(readFileSync(new URL("tauri.windows.conf.json", windows)));
  assert.equal(
    effective.build.beforeBundleCommand,
    "node os-integration/generate.mjs --check-windows-resources",
  );
});

test("NSIS keeps current-user classic verbs with matching uninstall hooks", () => {
  assert.equal(config.nsis.installMode, "currentUser");
  assert.equal(config.nsis.installerHooks, "windows/os-integration.nsh");
  const hooks = files["src-tauri/windows/os-integration.nsh"];
  assert.match(hooks, /!macro NSIS_HOOK_POSTINSTALL/);
  assert.match(hooks, /!macro NSIS_HOOK_POSTUNINSTALL/);
  assert.match(hooks, /WriteRegStr SHCTX/);
  assert.match(hooks, /DeleteRegKey SHCTX/);
  assert.doesNotMatch(hooks, /WriteReg\w+ HKLM/);
});
