// Entry point.
//   node src/main.ts          apply migrations, then serve
//   node src/main.ts serve    same as above
//   node src/main.ts migrate  apply migrations and exit

import { serve } from "@hono/node-server";
import { getConnInfo } from "@hono/node-server/conninfo";
import { createApp } from "./app.ts";
import {
  buildAuthOptions,
  createAuth,
  createPool,
  discoveryDocuments,
  scheduleLegacySigningKeys,
} from "./auth.ts";
import { CIMD_CLEANUP_INTERVAL_MS, cleanupUnusedCimdClients } from "./cimd.ts";
import { ConfigError, parseConfig } from "./config.ts";
import { migrate } from "./migrate.ts";
import { pageOptions } from "./pages.ts";

function log(level: "info" | "error", message: string, fields: Record<string, unknown> = {}) {
  const line = JSON.stringify({ time: new Date().toISOString(), level, message, ...fields });
  (level === "error" ? process.stderr : process.stdout).write(`${line}\n`);
}

async function main(command: string): Promise<void> {
  if (command !== "serve" && command !== "migrate") {
    throw new ConfigError(`unknown command "${command}"; expected serve or migrate`);
  }
  const config = parseConfig(process.env);
  const pool = createPool(config);
  pool.on("error", (err) =>
    log("error", "idle database connection failed", { error: err.message }),
  );

  const options = buildAuthOptions(config, pool);
  const summary = await migrate(pool, options);
  log("info", "auth schema is up to date", { ...summary });
  if (command === "migrate") {
    await pool.end();
    return;
  }

  await scheduleLegacySigningKeys(pool);
  let cleaning = false;
  const cleanCimd = async () => {
    if (cleaning) return;
    cleaning = true;
    try {
      await cleanupUnusedCimdClients((sql, parameters) => pool.query(sql, parameters));
    } catch (error) {
      log("error", "CIMD client cleanup failed", {
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      cleaning = false;
    }
  };
  const cleanupTimer = config.oauthDisabledReason
    ? undefined
    : setInterval(() => void cleanCimd(), CIMD_CLEANUP_INTERVAL_MS).unref();
  if (cleanupTimer) void cleanCimd();
  const auth = createAuth(options);
  if (config.oauthDisabledReason) {
    log("info", "remote MCP OAuth is off", { reason: config.oauthDisabledReason });
  }
  const app = createApp({
    config,
    handler: (request) => auth.handler(request),
    metadata: config.oauthDisabledReason ? undefined : discoveryDocuments(auth),
    pages: pageOptions(config),
    peerAddress: (c) => getConnInfo(c).remote.address,
  });
  const server = serve({ fetch: app.fetch, port: config.port, hostname: config.host }, (info) =>
    log("info", "auth service listening", {
      address: info.address,
      port: info.port,
      mcpResource: config.mcpResource,
    }),
  );

  const shutdown = (signal: string) => {
    log("info", "shutting down", { signal });
    if (cleanupTimer) clearInterval(cleanupTimer);
    server.close(() => {
      pool.end().finally(() => process.exit(0));
    });
    // Do not wait forever on keep-alive connections.
    setTimeout(() => process.exit(0), 10_000).unref();
  };
  process.once("SIGTERM", () => shutdown("SIGTERM"));
  process.once("SIGINT", () => shutdown("SIGINT"));
}

main(process.argv[2] ?? "serve").catch((err: unknown) => {
  const message = err instanceof Error ? err.message : String(err);
  log("error", err instanceof ConfigError ? "invalid configuration" : "auth service failed", {
    error: message,
  });
  process.exit(1);
});
