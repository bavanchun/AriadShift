// Applies Better Auth's schema to the `auth` schema. Runs before the server
// listens, and on its own through `node src/main.ts migrate`.

import { getMigrations } from "better-auth/db/migration";
import type pg from "pg";
import type { buildAuthOptions } from "./auth.ts";

/** Session advisory lock that serialises migrations across replicas. */
const MIGRATION_LOCK_KEY = 7_270_514_213_820_104_001n;

export interface MigrationSummary {
  createdTables: string[];
  addedColumns: number;
}

export async function migrate(
  pool: pg.Pool,
  options: ReturnType<typeof buildAuthOptions>,
): Promise<MigrationSummary> {
  // The lock lives on its own connection; the migration uses the pool's others.
  const lock = await pool.connect();
  try {
    await lock.query("SELECT pg_advisory_lock($1)", [MIGRATION_LOCK_KEY.toString()]);
    try {
      const plan = await getMigrations(options);
      if (plan.schemaProblems.length > 0) {
        const problems = plan.schemaProblems.join("\n");
        throw new Error(`the auth schema has unexpected columns:\n${problems}`);
      }
      await plan.runMigrations();
      return {
        createdTables: plan.toBeCreated.map((table) => table.table),
        addedColumns: plan.toBeAdded.reduce((sum, t) => sum + Object.keys(t.fields).length, 0),
      };
    } finally {
      await lock.query("SELECT pg_advisory_unlock($1)", [MIGRATION_LOCK_KEY.toString()]);
    }
  } finally {
    lock.release();
  }
}
