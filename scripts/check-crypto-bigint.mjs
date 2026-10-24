import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

// The advisory exclusion relies on every normal dependency path ending at the JWT verifier.
export function checkVerifierTree(tree) {
  const lines = tree.trim().split("\n");
  if (lines[0] !== "0crypto-bigint v0.5.5") {
    throw new Error("Expected crypto-bigint 0.5.5; reassess its advisory exclusion");
  }
  const path = [];
  const checkLeaf = () => {
    if (path.slice(-2).join("/") !== "jsonwebtoken/ariad-server") {
      throw new Error(`Unexpected crypto-bigint consumer: ${path.join(" -> ")}`);
    }
  };
  for (const line of lines) {
    const match = /^(\d+)(\S+) v\S+(?: \(.*\))?$/.exec(line);
    if (!match || Number(match[1]) > path.length) {
      throw new Error(`Invalid cargo tree entry: ${line}`);
    }
    const depth = Number(match[1]);
    if (path.length && depth < path.length) checkLeaf();
    path.length = depth;
    path.push(match[2]);
  }
  checkLeaf();
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const result = spawnSync(
    "cargo",
    [
      "tree",
      "--locked",
      "--workspace",
      "--all-features",
      "--target",
      "all",
      "--edges",
      "normal",
      "--invert",
      "crypto-bigint@0.5.5",
      "--no-dedupe",
      "--prefix",
      "depth",
      "--format",
      "{p}",
      "--color",
      "never",
    ],
    { encoding: "utf8" },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.stderr.write(result.stderr);
    process.exit(result.status ?? 1);
  }
  checkVerifierTree(result.stdout);
  console.log("crypto-bigint 0.5.5 is confined to jsonwebtoken under ariad-server");
}
