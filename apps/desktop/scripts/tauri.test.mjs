import assert from "node:assert/strict";
import { test } from "node:test";
import { signingInvocation } from "./tauri.mjs";

test("empty Apple secrets become absent without changing build or bundle arguments", () => {
  for (const command of ["build", "bundle"]) {
    const result = signingInvocation(
      [command, "--bundles", "app"],
      {
        APPLE_CERTIFICATE: "",
        APPLE_CERTIFICATE_PASSWORD: "",
        APPLE_SIGNING_IDENTITY: "",
        APPLE_ID: "",
        APPLE_PASSWORD: "",
        APPLE_TEAM_ID: "",
        TAURI_SIGNING_PRIVATE_KEY: "test-updater-key",
        PATH: "/bin",
      },
      "darwin",
    );
    assert.deepEqual(result.args, [command, "--bundles", "app"]);
    assert.deepEqual(result.env, { PATH: "/bin", TAURI_SIGNING_PRIVATE_KEY: "test-updater-key" });
  }
});

test("complete signing credentials preserve signing and reject partial groups", () => {
  const env = {
    APPLE_CERTIFICATE: "test-certificate",
    APPLE_CERTIFICATE_PASSWORD: "test-password",
    APPLE_SIGNING_IDENTITY: "test-identity",
  };
  assert.deepEqual(signingInvocation(["build"], env, "darwin").args, ["build"]);
  assert.throws(
    () => signingInvocation(["build"], { APPLE_CERTIFICATE: "test-certificate" }, "darwin"),
    /requires/,
  );
  assert.throws(
    () => signingInvocation(["build"], { ...env, APPLE_ID: "test-account" }, "darwin"),
    /together/,
  );
});

test("other platforms and development commands keep their signing behavior", () => {
  for (const [platform, command] of [
    ["linux", "build"],
    ["win32", "build"],
    ["darwin", "dev"],
  ]) {
    assert.deepEqual(signingInvocation([command], {}, platform).args, [command]);
  }
});
