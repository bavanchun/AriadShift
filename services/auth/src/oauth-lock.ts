import type pg from "pg";

/** Distinct from the migration lock; shared by every auth replica. */
export const OAUTH_TOKEN_LOCK_KEY = "7270514213820104002";

/** Paths whose token writes share the PostgreSQL session lock. */
export const SERIALIZED_OAUTH_PATHS: ReadonlySet<string> = new Set([
  "/api/auth/oauth2/token",
  "/api/auth/oauth2/revoke",
  "/api/auth/oauth2/delete-consent",
  "/api/auth/oauth2/delete-client",
]);

/** One trailing-slash rule for HTTP safeguards and token serialization. */
export function normalizeOAuthPath(path: string): string {
  return path.replace(/\/+$/, "") || "/";
}

/** Milliseconds, except capacity; tune together with the ten-connection pool. */
export const OAUTH_TOKEN_LIMITS = {
  capacity: 3,
  checkoutMs: 2_000,
  acquisitionMs: 3_000,
  statementMs: 5_000,
  lockMs: 2_000,
  requestMs: 10_000,
} as const;

// Share admission across handlers using the same pool, including test replicas.
const admitted = new WeakMap<pg.Pool, number>();
type Handler = (request: Request) => Promise<Response>;
export type OAuthTokenLimits = { [K in keyof typeof OAUTH_TOKEN_LIMITS]: number };

function unavailable() {
  return Response.json(
    { error: "temporarily_unavailable", error_description: "Please retry later." },
    {
      status: 503,
      headers: { "Retry-After": "1", "Cache-Control": "no-store", Pragma: "no-cache" },
    },
  );
}

/**
 * One backend owns the session lock and every provider query, even across its
 * internal transactions. Expiration destroys that backend; never unlock and
 * let an abandoned provider continue writing on a replacement connection.
 */
export function serializedOAuthHandler(
  pool: pg.Pool,
  handler: Handler,
  boundHandler: (database: pg.Pool) => Handler,
  limits: OAuthTokenLimits = OAUTH_TOKEN_LIMITS,
): Handler {
  return async (request) => {
    const path = normalizeOAuthPath(new URL(request.url).pathname);
    if (request.method !== "POST" || !SERIALIZED_OAUTH_PATHS.has(path)) {
      return handler(request);
    }

    // The HTTP layer caps auth bodies at 64 KiB. Read before reserving capacity,
    // a connection or the lock, so slow uploads cannot block token clients.
    const buffered = new Request(request, { body: await request.arrayBuffer() });
    const count = admitted.get(pool) ?? 0;
    if (count >= limits.capacity) return unavailable();
    admitted.set(pool, count + 1);

    const expires = Date.now() + limits.requestMs;
    let client: pg.PoolClient | undefined;
    let failed = false;
    let released = false;
    const destroy = () => {
      failed = true;
      if (client && !released) {
        released = true;
        // pg ends/destroys the socket and fails pending and future queries.
        // Server statement deadlines also cancel work if the socket disappears.
        const closing = client;
        closing.once("end", () => closing.removeListener("error", destroy));
        client.release(true);
      }
    };
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => {
        destroy();
        reject(new Error("OAuth request expired"));
      }, limits.requestMs);
    });
    const run = async () => {
      const acquired = await pool.connect();
      if (failed) {
        acquired.release(true);
        return unavailable();
      }
      client = acquired;
      client.on("error", destroy);
      await client.query(
        "SELECT set_config('statement_timeout', $1, false), set_config('lock_timeout', $1, false)",
        [String(limits.acquisitionMs)],
      );
      await client.query("SELECT pg_advisory_lock($1)", [OAUTH_TOKEN_LOCK_KEY]);

      // Kysely uses query(text, values). Serialize each settings/query pair so
      // even parallel provider calls inherit their own remaining server budget.
      let queue: Promise<unknown> = Promise.resolve();
      const query = (text: string, values?: unknown[]) => {
        const next = queue
          .then(async () => {
            const remaining = expires - Date.now();
            if (failed || released || remaining <= 0) throw new Error("OAuth request expired");
            await acquired.query(
              "SELECT set_config('statement_timeout', $1, false), set_config('lock_timeout', $2, false)",
              [
                String(Math.min(limits.statementMs, remaining)),
                String(Math.min(limits.lockMs, remaining)),
              ],
            );
            if (failed || Date.now() >= expires) throw new Error("OAuth request expired");
            return acquired.query(text, values);
          })
          .catch((error: unknown) => {
            destroy();
            throw error;
          });
        queue = next;
        return next;
      };
      const database = {
        connect: async () => ({ query, release() {} }),
        end: async () => {},
      } as unknown as pg.Pool;
      const response = await boundHandler(database)(buffered);
      await queue;
      if (failed) return unavailable();
      // Reset before recycling; failures (including aborted transactions) destroy
      // the session instead. No timeout setting leaks into ordinary sign-in.
      await acquired.query("RESET statement_timeout; RESET lock_timeout");
      await acquired.query("SELECT pg_advisory_unlock($1)", [OAUTH_TOKEN_LOCK_KEY]);
      return response;
    };
    try {
      return await Promise.race([run(), deadline]);
    } catch {
      destroy();
      return unavailable();
    } finally {
      if (timer) clearTimeout(timer);
      if (client && !released) {
        released = true;
        client.removeListener("error", destroy);
        client.release();
      }
      admitted.set(pool, (admitted.get(pool) ?? 1) - 1);
    }
  };
}
