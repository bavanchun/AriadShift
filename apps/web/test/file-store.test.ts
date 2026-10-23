import { holdLock, MEMORY_LIMIT_BYTES, OpfsStore, randomSessionId } from "@/converter/file-store";
import { INPUT_LIMIT_BYTES } from "@/converter/session";

describe("randomSessionId", () => {
  const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

  it("returns a valid RFC 4122 v4 UUID with default crypto", () => {
    const id = randomSessionId();
    expect(id).toMatch(uuidRegex);
  });

  it("derives a valid RFC 4122 v4 UUID from getRandomValues when randomUUID is unavailable", () => {
    // Simulate non-secure context (e.g. plain HTTP on LAN) where randomUUID does not exist
    const mockCryptoWithoutRandomUUID = {
      getRandomValues: (array: Uint8Array) => crypto.getRandomValues(array),
    } as unknown as Crypto;

    const id = randomSessionId(mockCryptoWithoutRandomUUID);
    expect(id).toMatch(uuidRegex);
  });

  it("generates distinct session IDs", () => {
    const id1 = randomSessionId();
    const id2 = randomSessionId();
    expect(id1).not.toBe(id2);
  });
});

describe("holdLock", () => {
  it("rejects through the request promise when locks.request rejects", async () => {
    const mockLocks = {
      request: vi.fn().mockRejectedValue(new Error("Lock request rejected")),
    } as unknown as LockManager;

    await expect(holdLock(mockLocks, "test-session")).rejects.toThrow("Lock request rejected");
  });

  it("resolves when the lock callback is called", async () => {
    let lockAcquired = false;
    const mockLocks = {
      request: vi.fn().mockImplementation((_name, callback) => {
        lockAcquired = true;
        return callback();
      }),
    } as unknown as LockManager;

    await holdLock(mockLocks, "test-session");
    expect(lockAcquired).toBe(true);
  });
});

describe("OpfsStore.open", () => {
  it("returns null in non-browser environments", async () => {
    const store = await OpfsStore.open("test-session");
    expect(store).toBeNull();
  });
});

describe("memory fallback effective limit", () => {
  it("calculates the effective limit as the smaller of memory limit and input limit", () => {
    const effectiveLimit = Math.min(MEMORY_LIMIT_BYTES, INPUT_LIMIT_BYTES);
    expect(effectiveLimit).toBe(16 * 1024 * 1024);
    expect(effectiveLimit).toBeLessThan(MEMORY_LIMIT_BYTES);
  });
});
