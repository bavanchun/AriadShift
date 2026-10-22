// Runs the pinned Lighthouse against the production build: / and /app, mobile and
// desktop. Starts `next start` on port 3100 itself and stops it afterwards. Exits non-zero
// when any category scores below 90. Run `just web-build` first.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import * as chromeLauncher from "chrome-launcher";
import lighthouse from "lighthouse";
import desktopConfig from "lighthouse/core/config/desktop-config.js";

const PORT = 3100;
const BASE = `http://localhost:${PORT}`;
const PAGES = ["/", "/app"];
const CATEGORIES = ["performance", "accessibility", "best-practices", "seo"];
const THRESHOLD = 0.9;
const web = path.join(import.meta.dirname, "..");
const reports = path.join(web, "../../.lighthouse");

async function waitForServer() {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      if ((await fetch(BASE)).ok) {
        return;
      }
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`next start did not answer on ${BASE}`);
}

const server = spawn("pnpm", ["exec", "next", "start", "--port", String(PORT)], {
  cwd: web,
  stdio: "ignore",
  shell: process.platform === "win32",
  detached: process.platform !== "win32",
});
const stopServer = () => {
  try {
    process.platform === "win32" ? server.kill() : process.kill(-server.pid, "SIGTERM");
  } catch {
    // Already stopped.
  }
};

let failed = false;
const rows = [];
try {
  await waitForServer();
  mkdirSync(reports, { recursive: true });
  for (const formFactor of ["mobile", "desktop"]) {
    for (const page of PAGES) {
      const chrome = await chromeLauncher.launch({ chromeFlags: ["--headless=new"] });
      try {
        const result = await lighthouse(
          `${BASE}${page}`,
          { port: chrome.port, output: "json", logLevel: "error", onlyCategories: CATEGORIES },
          formFactor === "desktop" ? desktopConfig : undefined,
        );
        const lhr = result.lhr;
        const scores = CATEGORIES.map((id) => lhr.categories[id].score);
        rows.push({ page, formFactor, scores });
        if (scores.some((score) => score === null || score < THRESHOLD)) {
          failed = true;
        }
        const name = `${formFactor}-${page === "/" ? "home" : page.slice(1)}.json`;
        writeFileSync(path.join(reports, name), result.report);
      } finally {
        chrome.kill();
      }
    }
  }
} finally {
  stopServer();
}

console.log("Lighthouse scores:");
console.log("| Page | Form factor | Performance | Accessibility | Best practices | SEO |");
console.log("|---|---|---:|---:|---:|---:|");
for (const { page, formFactor, scores } of rows) {
  console.log(
    `| \`${page}\` | ${formFactor} | ${scores.map((score) => Math.round((score ?? 0) * 100)).join(" | ")} |`,
  );
}
console.log(`Reports: ${reports}`);
process.exit(failed ? 1 : 0);
