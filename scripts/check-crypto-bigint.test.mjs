import assert from "node:assert/strict";
import { test } from "node:test";
import { checkVerifierTree } from "./check-crypto-bigint.mjs";

const verifierTree = `0crypto-bigint v0.5.5
1elliptic-curve v0.13.8
2p256 v0.13.2
3jsonwebtoken v11.1.0
4ariad-server v0.1.0 (/workspace/crates/ariad-server)
2p384 v0.13.1
3jsonwebtoken v11.1.0
4ariad-server v0.1.0 (/workspace/crates/ariad-server)`;

test("accepts every path through the server JWT verifier", () => {
  assert.doesNotThrow(() => checkVerifierTree(verifierTree));
});

test("rejects an additional consumer even when the verifier paths remain", () => {
  for (const branch of [
    "1ariad-server v0.1.0",
    "1another-consumer v1.0.0\n2ariad-server v0.1.0",
    "1jsonwebtoken v11.1.0\n2ariad-desktop v0.1.0",
    "5ariad-desktop v0.1.0",
  ]) {
    assert.throws(() => checkVerifierTree(`${verifierTree}\n${branch}`), /Unexpected/);
  }
});

test("requires the excluded version and a complete dependency tree", () => {
  for (const tree of [
    "",
    verifierTree.replace("v0.5.5", "v0.7.6"),
    "0crypto-bigint v0.5.5",
    verifierTree.replace("2p256", "3p256"),
    `${verifierTree}\n1elliptic-curve v0.13.8 (*)`,
  ]) {
    assert.throws(() => checkVerifierTree(tree));
  }
});
