import assert from "node:assert/strict";
import { test } from "node:test";
import { signingInvocation } from "./tauri.mjs";

test("empty Apple secrets become absent and macOS builds explicitly skip signing", () => {
  const result = signingInvocation(
    ["build", "--bundles", "app"],
    {
      APPLE_CERTIFICATE: "",
      APPLE_CERTIFICATE_PASSWORD: "",
      APPLE_SIGNING_IDENTITY: "",
      PATH: "/bin",
    },
    "darwin",
  );
  assert.deepEqual(result.args, ["build", "--bundles", "app", "--no-sign"]);
  assert.deepEqual(result.env, { PATH: "/bin" });
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
  assert.deepEqual(signingInvocation(["build", "--no-sign"], {}, "darwin").args, [
    "build",
    "--no-sign",
  ]);
});
