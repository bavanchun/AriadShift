import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { verifyTool } from "./linux-tools.mjs";

test("bundler executables require the exact reviewed digest", () => {
  const bytes = Buffer.from("reviewed executable");
  const tool = { name: "tool", sha256: createHash("sha256").update(bytes).digest("hex") };
  verifyTool(bytes, tool);
  assert.throws(() => verifyTool(Buffer.from("changed executable"), tool), /SHA-256 mismatch/);
});
