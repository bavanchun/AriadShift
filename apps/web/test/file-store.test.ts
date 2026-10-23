import { holdLock, OpfsStore, randomSessionId } from "@/converter/file-store";

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
  it("returns null when acquiring the session lock rejects", async () => {
    const originalNavigator = globalThis.navigator;
    const originalFileSystemFileHandle = globalThis.FileSystemFileHandle;

    class FakeFileSystemFileHandle {
      createSyncAccessHandle() {}
    }

    const mockLocks = {
      request: vi.fn().mockRejectedValue(new Error("Lock request rejected")),
    } as unknown as LockManager;

    const mockStorage = {
      getDirectory: vi.fn(),
    } as unknown as StorageManager;

    try {
      Object.defineProperty(globalThis, "navigator", {
        value: {
          locks: mockLocks,
          storage: mockStorage,
        },
        configurable: true,
        writable: true,
      });
      Object.defineProperty(globalThis, "FileSystemFileHandle", {
        value: FakeFileSystemFileHandle,
        configurable: true,
        writable: true,
      });

      const store = await OpfsStore.open("test-session");
      expect(store).toBeNull();
      expect(mockLocks.request).toHaveBeenCalledWith(
        "ariadshift:test-session",
        expect.any(Function),
      );
    } finally {
      if (originalNavigator !== undefined) {
        Object.defineProperty(globalThis, "navigator", {
          value: originalNavigator,
          configurable: true,
          writable: true,
        });
      } else {
        delete (globalThis as Record<string, unknown>).navigator;
      }
      if (originalFileSystemFileHandle !== undefined) {
        Object.defineProperty(globalThis, "FileSystemFileHandle", {
          value: originalFileSystemFileHandle,
          configurable: true,
          writable: true,
        });
      } else {
        delete (globalThis as Record<string, unknown>).FileSystemFileHandle;
      }
    }
  });
});
