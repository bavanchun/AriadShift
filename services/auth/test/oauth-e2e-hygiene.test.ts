import { execFile, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { expect, it } from "vitest";

const oauth = readFileSync(
  new URL("../../../infra/test/e2e/mcp-oauth.sh", import.meta.url),
  "utf8",
);
const ordinary = readFileSync(new URL("../../../infra/test/e2e/run.sh", import.meta.url), "utf8");

it("keeps OAuth credentials out of external command arguments", () => {
  for (const leak of [
    /-H "authorization: Bearer \$/,
    /--arg p "\$PASSWORD"/,
    /--data "\$\(jq[^\n]*password/,
    /"code=\$CODE"/,
    /"code_verifier=\$VERIFIER"/,
    /"refresh_token=\$REFRESH_TOKEN"/,
    /"code_challenge=\$VERIFIER"/,
    /--arg u "\$1"/,
  ])
    expect(oauth, String(leak)).not.toMatch(leak);
  expect(oauth).toContain("umask 077");
  expect(oauth).toContain('rm -rf "$WORK"');
  expect(oauth).toContain('code@$(private_file code "$CODE")');
  expect(oauth).toContain('code_verifier@$(private_file verifier "$VERIFIER")');
  expect(oauth).toContain('refresh_token@$(private_file refresh "$REFRESH_TOKEN")');
  expect(oauth).not.toMatch(
    /fail "[^"\n]*\$(SIGN_IN_URL|CONSENT_URL|CALLBACK|CODE|VERIFIER|ACCESS_TOKEN|REFRESH_TOKEN)/,
  );
});

it("resets only its owned project before starting the stack", () => {
  const stack = oauth.indexOf("# --- Stack");
  const reset = oauth.indexOf("reset_project", stack);
  expect(stack).toBeGreaterThan(-1);
  expect(reset).toBeGreaterThan(stack);
  expect(reset).toBeLessThan(oauth.indexOf("compose up -d", stack));
  expect(oauth).toContain("com.docker.compose.project.working_dir");
  expect(oauth).toContain("ariadshift.test-worktree");
});

it.each([
  { label: "/test/worktree", mounted: "retained-volume", accepted: true },
  { label: "<no value>", mounted: "retained-volume", accepted: true },
  { label: "/foreign/worktree", mounted: "retained-volume", accepted: false },
  { label: "<no value>", mounted: "", accepted: false },
])("checks volume ownership for $label and mount '$mounted'", ({ label, mounted, accepted }) => {
  const helper = oauth.slice(
    oauth.indexOf("assert_owned_project() {"),
    oauth.indexOf("reset_project() {"),
  );
  // Model Docker's read-only metadata responses. No container is deleted by
  // this policy test; the real Compose gate exercises reset and teardown.
  const result = spawnSync(
    "bash",
    [
      "-c",
      `
set -euo pipefail
fail() { exit 1; }
docker() {
  case "$1 $2" in
    'ps -aq') printf 'owned-container\\n' ;;
    'inspect -f')
      case "$3" in
        *working_dir*) printf '/test/worktree/infra/compose\\n' ;;
        *) printf '%s\\n' "$MOUNTED" ;;
      esac ;;
    'volume ls') printf 'retained-volume\\n' ;;
    'volume inspect') printf '%s\\n' "$LABEL" ;;
    *) exit 99 ;;
  esac
}
${helper}
assert_owned_project
`,
    ],
    {
      env: {
        ...process.env,
        ROOT: "/test/worktree",
        COMPOSE_FILE: "/test/worktree/infra/compose/docker-compose.yml",
        PROJECT: "ariadshift-policy-e2e",
        LABEL: label,
        MOUNTED: mounted,
      },
      encoding: "utf8",
    },
  );
  expect(result.status, result.stderr).toBe(accepted ? 0 : 1);
});

it.each([
  { name: "OAuth", script: oauth },
  { name: "ordinary Compose", script: ordinary },
])("explicitly opts $name into signup", ({ script }) => {
  expect(script).toContain("AUTH_ALLOW_SIGN_UP=true");
  expect(script.indexOf("AUTH_ALLOW_SIGN_UP=true")).toBeLessThan(script.indexOf("# --- Stack"));
});

it("sends private credential files through real curl without argv exposure", async () => {
  const directory = mkdtempSync(join(tmpdir(), "oauth-hygiene-"));
  const secret = "synthetic-secret-with+reserved/characters=";
  const received: { url?: string; authorization?: string; body?: string } = {};
  const server = createServer((request, response) => {
    received.url = request.url;
    received.authorization = request.headers.authorization;
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => {
      received.body = body;
      response.end("ok");
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("missing test listener");
    const privateHelper = oauth.slice(
      oauth.indexOf("private_file() {"),
      oauth.indexOf("random_hex() {"),
    );
    const requestHelper = oauth.slice(
      oauth.indexOf("request() {"),
      oauth.indexOf("# Never print raw"),
    );
    const mcpHelper = oauth.slice(
      oauth.indexOf("mcp() {"),
      oauth.indexOf("# --- Throwaway environment"),
    );
    const { stdout } = await promisify(execFile)(
      "bash",
      [
        "-c",
        `
set -euo pipefail
umask 077
BODY="$WORK/body" HEADERS="$WORK/headers"
${privateHelper}
${requestHelper}
${mcpHelper}
curl() { printf '%s\\n' "$@" >"$WORK/argv"; command curl "$@"; }
RESOURCE="$ENDPOINT/?code=$SECRET" MCP_VERSION=2026-07-28
mcp "$SECRET" tools/list
`,
      ],
      {
        env: {
          ...process.env,
          WORK: directory,
          SECRET: secret,
          ENDPOINT: `http://127.0.0.1:${address.port}`,
        },
      },
    );
    expect(stdout).toBe("200");
    expect(received.authorization).toBe(`Bearer ${secret}`);
    expect(JSON.parse(received.body ?? "")).toMatchObject({ method: "tools/list" });
    expect(received.url).toContain(secret);
    expect(readFileSync(join(directory, "argv"), "utf8")).not.toContain(secret);
    expect(statSync(join(directory, "bearer")).mode & 0o777).toBe(0o600);
    expect(statSync(join(directory, "request.conf")).mode & 0o777).toBe(0o600);
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
    rmSync(directory, { recursive: true, force: true });
  }
});
