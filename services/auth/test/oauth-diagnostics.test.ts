import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";

it("allows only safe error fields and omits malformed e2e response bodies", () => {
  const script = readFileSync(
    new URL("../../../infra/test/e2e/mcp-oauth.sh", import.meta.url),
    "utf8",
  );
  const helper = script.slice(
    script.indexOf("diagnostic_body() {"),
    script.indexOf("# expect WANT GOT WHAT"),
  );
  const directory = mkdtempSync(join(tmpdir(), "auth-diagnostics-"));
  const body = join(directory, "body");
  const sensitive = [
    "access_token",
    "refresh_token",
    "id_token",
    "client_secret",
    "sessionToken",
    "token",
  ];
  try {
    writeFileSync(
      body,
      JSON.stringify({
        status: 503,
        error: "temporarily_unavailable",
        error_description: "synthetic-description-secret",
        nested: Object.fromEntries(sensitive.map((key) => [key, `credential-${key}`])),
      }),
    );
    const diagnostic = () =>
      execFileSync("bash", ["-c", `set -o pipefail\n${helper}\ndiagnostic_body`], {
        env: { ...process.env, BODY: body },
        encoding: "utf8",
      });
    const output = diagnostic();
    expect(JSON.parse(output)).toEqual({ status: 503, error: "temporarily_unavailable" });
    expect(output).not.toContain("synthetic-description-secret");
    for (const key of sensitive) expect(output).not.toContain(`credential-${key}`);

    writeFileSync(body, "session_token=must-never-appear");
    expect(diagnostic()).toBe("[response omitted]");
    for (const value of [
      '"synthetic-scalar-secret" malformed',
      '{"error":"invalid_grant"} malformed',
      '"synthetic-scalar-secret"',
      '["synthetic-array-secret", {"access_token":"synthetic-nested-secret"}]',
      '{"error":"invalid_grant"} {"status":503}',
      "42",
      "null",
    ]) {
      writeFileSync(body, value);
      expect(diagnostic()).toBe("[response omitted]");
    }
    writeFileSync(
      body,
      JSON.stringify({ error: "synthetic-error-secret", status: "synthetic-status-secret" }),
    );
    expect(diagnostic()).toBe("{}");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
