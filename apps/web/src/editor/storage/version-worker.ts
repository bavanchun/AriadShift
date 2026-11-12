import { OpfsVersionBackend } from "./opfs-version-backend";
import type { VersionWorkerReply, VersionWorkerRequest } from "./types";

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<VersionWorkerRequest>) => void) | null;
  postMessage(value: VersionWorkerReply): void;
};
let backend: OpfsVersionBackend | null = null;

async function open(): Promise<boolean> {
  const root = await navigator.storage.getDirectory();
  const name = `ariadshift-editor-probe-${crypto.randomUUID()}`;
  const file = await root.getFileHandle(name, { create: true });
  try {
    if (!("createSyncAccessHandle" in file)) return false;
    const handle = await (
      file as FileSystemFileHandle & {
        createSyncAccessHandle(): Promise<{ close(): void }>;
      }
    ).createSyncAccessHandle();
    handle.close();
  } finally {
    await root.removeEntry(name);
  }
  backend = new OpfsVersionBackend(root);
  return true;
}

async function execute(request: VersionWorkerRequest): Promise<unknown> {
  if (request.method === "open") return open();
  if (!backend) throw new Error("Editor OPFS backend is unavailable");
  switch (request.method) {
    case "list":
      return backend.list(request.docId);
    case "read":
      return backend.read(request.docId, request.versionId ?? "");
    case "write":
      if (!request.version) throw new Error("Missing editor version payload");
      return backend.write(request.docId, request.version);
    case "remove":
      return backend.remove(request.docId, request.versionId ?? "");
    case "purge":
      return backend.purge(request.docId);
  }
}

scope.onmessage = (event) => {
  const request = event.data;
  execute(request).then(
    (result) => scope.postMessage({ requestId: request.requestId, result }),
    (error: unknown) =>
      scope.postMessage({
        requestId: request.requestId,
        error: error instanceof Error ? error.message : "Editor version operation failed",
      }),
  );
};
