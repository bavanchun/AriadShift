import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const cli = join(dirname(require.resolve("vitest/package.json")), "vitest.mjs");

describe("required database execution", () => {
  it.each([undefined, "", "   "])(
    "fails the real PostgreSQL suite for absent or blank URL %j",
    (value) => {
      const env: Record<string, string | undefined> = {
        ...process.env,
        ARIAD_TEST_REQUIRE_DATABASE: "1",
        // The unfixed suite must never fall back to a developer's database.
        PGHOST: "/dev/null",
        PGPORT: "1",
        PGDATABASE: "unused",
        PGUSER: "unused",
        PGPASSWORD: "",
      };
      delete env.ARIAD_TEST_DATABASE_URL;
      if (value !== undefined) env.ARIAD_TEST_DATABASE_URL = value;
      const result = spawnSync(process.execPath, [cli, "run", "test/oauth-postgres.test.ts"], {
        cwd: fileURLToPath(new URL("../", import.meta.url)),
        env,
        encoding: "utf8",
        timeout: 30_000,
      });
      expect(result.error).toBeUndefined();
      expect(result.status).not.toBe(0);
      expect(result.stdout + result.stderr).toContain(
        "ARIAD_TEST_REQUIRE_DATABASE=1 needs ARIAD_TEST_DATABASE_URL",
      );
    },
    35_000,
  );
});
