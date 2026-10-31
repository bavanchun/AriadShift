import assert from "node:assert/strict";
import { test } from "node:test";
import { releaseTargets } from "./verify-release.mjs";

const platforms = ["linux-x86_64", "darwin-aarch64", "windows-x86_64"];
function fixture() {
  const assets = platforms.map((name, id) => ({
    id,
    name,
    browser_download_url: `https://github.com/example/project/releases/download/v1.2.3/${name}`,
  }));
  const metadata = {
    version: "1.2.3",
    platforms: Object.fromEntries(
      assets.map((asset) => [
        asset.name,
        { url: asset.browser_download_url, signature: "unit-test-selection-only" },
      ]),
    ),
  };
  return { assets, metadata };
}
test("all updater platforms must reference distinct attached release assets", () => {
  const { metadata, assets } = fixture();
  assert.equal(releaseTargets(metadata, assets, "v1.2.3").length, 3);
  assert.throws(() => releaseTargets(metadata, assets, "v1.2.4"), /version/);
  delete metadata.platforms["darwin-aarch64"];
  assert.throws(() => releaseTargets(metadata, assets, "v1.2.3"), /darwin-aarch64/);
});
test("missing signatures, external URLs and lost matrix updates fail closed", () => {
  for (const mutate of [
    (entry) => {
      entry.signature = "";
    },
    (entry) => {
      entry.url = "https://elsewhere.invalid/update";
    },
    (entry, metadata) => {
      metadata.platforms["windows-x86_64"] = entry;
    },
  ]) {
    const { metadata, assets } = fixture();
    mutate(metadata.platforms["linux-x86_64"], metadata);
    assert.throws(() => releaseTargets(metadata, assets, "v1.2.3"));
  }
});
