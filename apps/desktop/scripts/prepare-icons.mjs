// Compile the existing brand layers with Apple's asset compiler on macOS.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
mkdirSync(path.join(desktop, "public"), { recursive: true });
copyFileSync(
  path.join(root, "brand/png/ariadshift-app-icon-1024.png"),
  path.join(desktop, "public/icon.png"),
);
if (process.platform === "darwin") {
  const working = path.join(root, ".tools/desktop-icon");
  const icon = path.join(working, "AriadShift.icon");
  rmSync(working, { recursive: true, force: true });
  mkdirSync(path.join(icon, "Assets"), { recursive: true });
  const files = readdirSync(path.join(root, "brand/icon-composer"))
    .filter((f) => f.endsWith(".svg"))
    .sort();
  for (const file of files)
    copyFileSync(path.join(root, "brand/icon-composer", file), path.join(icon, "Assets", file));
  const manifest = {
    fill: "automatic",
    groups: [
      {
        layers: files.map((file, index) => ({
          name: file.replace(".svg", ""),
          "image-name": file,
          glass: index > 0,
          hidden: false,
          "blend-mode": "normal",
        })),
        shadow: { kind: "neutral", opacity: 0.5 },
        translucency: { enabled: true, value: 0.5 },
      },
    ],
    "supported-platforms": { circles: ["watchOS"], squares: "shared" },
  };
  writeFileSync(path.join(icon, "icon.json"), JSON.stringify(manifest, null, 2));
  const output = path.join(desktop, "src-tauri/icons/macos");
  mkdirSync(output, { recursive: true });
  execFileSync(
    "xcrun",
    [
      "actool",
      icon,
      "--compile",
      output,
      "--output-format",
      "human-readable-text",
      "--notices",
      "--warnings",
      "--errors",
      "--output-partial-info-plist",
      path.join(working, "partial.plist"),
      "--app-icon",
      "AriadShift",
      "--include-all-app-icons",
      "--enable-on-demand-resources",
      "NO",
      "--development-region",
      "en",
      "--target-device",
      "mac",
      "--minimum-deployment-target",
      "12.0",
      "--platform",
      "macosx",
    ],
    { stdio: "inherit" },
  );
}
