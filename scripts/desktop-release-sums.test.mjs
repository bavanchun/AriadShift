import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  checksumFile,
  collectLocal,
  isDesktopAsset,
  productName,
  requireAllKinds,
  SUMS_FILE,
  selectAssets,
} from "./desktop-release-sums.mjs";

const releaseNames = [
  "AriadShift_1.0.0_amd64.AppImage",
  "AriadShift_1.0.0_amd64.AppImage.sig",
  "AriadShift_1.0.0_amd64.deb",
  "AriadShift-1.0.0-1.x86_64.rpm",
  "AriadShift_1.0.0_aarch64.dmg",
  "AriadShift_1.0.0_aarch64.app.tar.gz",
  "AriadShift_1.0.0_aarch64.app.tar.gz.sig",
  "AriadShift_1.0.0_x64-setup.exe",
  "AriadShift_1.0.0_x64-setup.exe.sig",
  "AriadShift_1.0.0_x64_en-US.msi",
  "latest.json",
];

test("selects desktop installers, updater artifacts and metadata only", () => {
  const cli = [
    "ariad-cli-x86_64-unknown-linux-musl.tar.xz",
    "ariad-cli-x86_64-unknown-linux-musl.tar.xz.sha256",
    "ariad-cli-installer.sh",
    "ashift.rb",
    "dist-manifest.json",
    "ariad-core.cdx.xml",
    SUMS_FILE,
    "AriadShift_1.0.0_amd64.AppImage.minisig",
    `${SUMS_FILE}.minisig`,
    "AriadShift_notes.txt",
  ];
  assert.deepEqual(selectAssets([...cli, ...releaseNames], "AriadShift"), [...releaseNames].sort());
});

test("matches the product name case-sensitively", () => {
  assert.equal(isDesktopAsset("ariadshift_1.0.0_amd64.deb", "AriadShift"), false);
  assert.equal(isDesktopAsset("AriadShift_1.0.0_amd64.deb.sig", "AriadShift"), true);
  assert.equal(isDesktopAsset("AriadShift_1.0.0_amd64.deb.minisig", "AriadShift"), false);
});

test("rejects empty selections, duplicates and unsafe names", () => {
  assert.throws(() => selectAssets(["ashift.rb"], "AriadShift"), /No desktop installers/);
  assert.throws(
    () => selectAssets(["AriadShift.deb", "AriadShift.deb"], "AriadShift"),
    /Duplicate desktop asset/,
  );
  assert.throws(
    () => selectAssets(["AriadShift 1.0.0.deb"], "AriadShift"),
    /Unsafe desktop asset name/,
  );
});

test("requires every platform installer, updater artifact and latest.json", () => {
  assert.doesNotThrow(() => requireAllKinds(releaseNames));
  const withoutWindows = releaseNames.filter((name) => !name.includes("x64"));
  assert.throws(() => requireAllKinds(withoutWindows), /-setup\.exe, \.msi/);
  const withoutMetadata = releaseNames.filter((name) => name !== "latest.json");
  assert.throws(() => requireAllKinds(withoutMetadata), /latest\.json/);
  const signaturesOnly = releaseNames.map((name) => (name.endsWith(".sig") ? name : `${name}.sig`));
  assert.throws(() => requireAllKinds(signaturesOnly), /\.AppImage/);
});

test("writes sorted GNU coreutils checksum lines", () => {
  const a = "a".repeat(64);
  const b = "b".repeat(64);
  assert.equal(
    checksumFile([
      { name: "latest.json", digest: b },
      { name: "AriadShift.deb", digest: a },
    ]),
    `${a}  AriadShift.deb\n${b}  latest.json\n`,
  );
  assert.throws(() => checksumFile([{ name: "x", digest: "ABC" }]), /Invalid SHA-256/);
});

test("reads the product name from the Tauri config", () => {
  assert.equal(productName(), "AriadShift");
});

test("collects a local bundle tree and hashes the copies", () => {
  const root = mkdtempSync(join(tmpdir(), "desktop-release-sums-"));
  try {
    const bundle = join(root, "bundle");
    for (const [kind, name, body] of [
      ["deb", "AriadShift_1.0.0_amd64.deb", "deb"],
      ["appimage", "AriadShift_1.0.0_amd64.AppImage", "appimage"],
      ["appimage", "build_appimage.sh", "script"],
      ["appimage/AriadShift.AppDir", "AriadShift.deb", "nested"],
    ]) {
      mkdirSync(join(bundle, kind), { recursive: true });
      writeFileSync(join(bundle, kind, name), body);
    }
    const out = join(root, "out");
    const entries = collectLocal(bundle, out, "AriadShift");
    assert.deepEqual(
      entries.map(({ name }) => name),
      ["AriadShift_1.0.0_amd64.AppImage", "AriadShift_1.0.0_amd64.deb"],
    );
    const sha256 = (body) => createHash("sha256").update(body).digest("hex");
    assert.equal(
      readFileSync(join(out, SUMS_FILE), "utf8"),
      `${sha256("appimage")}  AriadShift_1.0.0_amd64.AppImage\n` +
        `${sha256("deb")}  AriadShift_1.0.0_amd64.deb\n`,
    );
    assert.equal(readFileSync(join(out, "AriadShift_1.0.0_amd64.deb"), "utf8"), "deb");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
