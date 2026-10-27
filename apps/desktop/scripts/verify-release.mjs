import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export function releaseTargets(metadata, assets, tag) {
  if (metadata.version !== tag.replace(/^v/, ""))
    throw new Error("Updater version differs from tag");
  const targets = ["linux-x86_64", "darwin-aarch64", "windows-x86_64"];
  const seen = new Set();
  return targets.map((platform) => {
    const entry = metadata.platforms?.[platform];
    const asset =
      typeof entry?.url === "string" &&
      entry.url.length > 0 &&
      assets.find(
        (candidate) => candidate.url === entry.url || candidate.browser_download_url === entry.url,
      );
    if (!asset || !entry.signature || seen.has(asset.id)) {
      throw new Error(`Missing, unattached or duplicate updater artifact for ${platform}`);
    }
    seen.add(asset.id);
    return { platform, asset, signature: entry.signature };
  });
}

export function verifySignature(artifact, signature, publicKey, directory) {
  const keyPath = join(directory, "updater.pub");
  const signaturePath = join(directory, "updater.sig");
  writeFileSync(keyPath, Buffer.from(publicKey, "base64"));
  writeFileSync(signaturePath, Buffer.from(signature, "base64"));
  execFileSync("minisign", ["-Vm", artifact, "-p", keyPath, "-x", signaturePath], {
    stdio: "pipe",
  });
}

export function verifyRelease(repository, tag) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !/^v\d+\.\d+\.\d+$/.test(tag)) {
    throw new Error("Expected a repository and a stable vX.Y.Z release tag");
  }
  const gh = (path) => execFileSync("gh", ["api", path], { encoding: "utf8" });
  const release = JSON.parse(gh(`repos/${repository}/releases/tags/${tag}`));
  const metadataAsset = release.assets.find((asset) => asset.name === "latest.json");
  if (!metadataAsset) throw new Error("Release has no latest.json");
  const download = (asset) =>
    execFileSync(
      "gh",
      [
        "api",
        `repos/${repository}/releases/assets/${asset.id}`,
        "-H",
        "Accept: application/octet-stream",
      ],
      { maxBuffer: 512 * 1024 * 1024 },
    );
  const targets = releaseTargets(JSON.parse(download(metadataAsset)), release.assets, tag);
  const config = JSON.parse(
    readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
  );
  const directory = mkdtempSync(join(tmpdir(), "ariad-updater-verify-"));
  try {
    for (const [index, target] of targets.entries()) {
      const artifact = join(directory, `artifact-${index}`);
      writeFileSync(artifact, download(target.asset));
      verifySignature(artifact, target.signature, config.plugins.updater.pubkey, directory);
      console.log(`Verified ${target.platform}: ${target.asset.name}`);
      rmSync(artifact);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  verifyRelease(process.env.GITHUB_REPOSITORY, process.env.RELEASE_TAG);
}
