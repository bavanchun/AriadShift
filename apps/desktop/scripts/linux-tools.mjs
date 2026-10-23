// Cache names match tauri-bundler 2.10.4. Reverify the manifest when upgrading it.
import { createHash } from "node:crypto";
import { chmod, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export function verifyTool(bytes, tool) {
  if (createHash("sha256").update(bytes).digest("hex") !== tool.sha256) {
    throw new Error(`SHA-256 mismatch for ${tool.name}; refusing to cache executable`);
  }
}

export async function prepareTools(directory) {
  const tools = JSON.parse(await readFile(new URL("./linux-tools.json", import.meta.url), "utf8"));
  await mkdir(directory, { recursive: true });
  for (const tool of tools) {
    const destination = join(directory, tool.name);
    let bytes = await readFile(destination).catch((error) => {
      if (error.code !== "ENOENT") throw error;
      return null;
    });
    if (bytes === null) {
      const response = await fetch(tool.url);
      if (!response.ok) throw new Error(`Cannot download ${tool.name}: ${response.status}`);
      bytes = Buffer.from(await response.arrayBuffer());
      verifyTool(bytes, tool);
      await writeFile(`${destination}.download`, bytes, { mode: 0o755 });
      await rename(`${destination}.download`, destination);
    } else {
      verifyTool(bytes, tool);
    }
    await chmod(destination, 0o755);
    console.log(`Verified ${tool.name}: ${tool.sha256}`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await prepareTools(
    process.argv[2] ?? join(process.env.XDG_CACHE_HOME || join(homedir(), ".cache"), "tauri"),
  );
}
