// Sends a file to the one presigned PUT that `POST /v1/uploads` returned, with
// Uppy's `@uppy/aws-s3` plugin in single-request mode. The URL is the only
// credential: Uppy signs nothing itself and never sees the API's JWT.
import { AriadApiError, type PresignedRequest } from "@ariadshift/sdk";
import AwsS3 from "@uppy/aws-s3";
import Uppy from "@uppy/core";

export interface TransferOptions {
  onProgress?: (sent: number, total: number) => void;
  signal?: AbortSignal;
}

/** Sends `file` to `presigned`; a progress callback reports bytes as they leave. */
export type Transfer = (
  file: File,
  presigned: PresignedRequest,
  options?: TransferOptions,
) => Promise<void>;

function aborted(): DOMException {
  return new DOMException("The upload was canceled.", "AbortError");
}

export const uppyTransfer: Transfer = async (file, presigned, options = {}) => {
  const { onProgress, signal } = options;
  if (presigned.method.toUpperCase() !== "PUT") {
    throw new AriadApiError(0, "transfer_failed", `unsupported upload method ${presigned.method}`);
  }
  if (signal?.aborted) {
    throw aborted();
  }

  const uppy = new Uppy({
    autoProceed: false,
    allowMultipleUploadBatches: false,
    restrictions: { maxNumberOfFiles: 1 },
  });
  uppy.use(AwsS3, {
    shouldUseMultipart: false,
    limit: 1,
    allowedMetaFields: false,
    // The server already signed the single PutObject request, bound to the
    // file's SHA-256 through the headers it returned.
    async signRequest(request) {
      if (request.method !== "PUT" || "uploadId" in request) {
        throw new Error("only a single presigned PUT is supported");
      }
      return { url: presigned.url, headers: presigned.headers };
    },
  });
  uppy.addFile({ name: file.name, type: file.type || undefined, data: file, source: "ariadshift" });
  uppy.on("upload-progress", (_file, progress) => {
    onProgress?.(progress.bytesUploaded, progress.bytesTotal ?? file.size);
  });

  let cancel: () => void = () => undefined;
  const canceled = new Promise<never>((_, reject) => {
    cancel = () => {
      uppy.cancelAll();
      reject(aborted());
    };
  });
  signal?.addEventListener("abort", cancel, { once: true });
  try {
    const result = await Promise.race([uppy.upload(), canceled]);
    const failed = result?.failed?.[0];
    if (failed || !result?.successful?.length) {
      const status = failed?.response?.status ?? 0;
      throw new AriadApiError(
        status,
        "transfer_failed",
        failed?.error ? String(failed.error) : "the upload did not finish",
      );
    }
    onProgress?.(file.size, file.size);
  } finally {
    signal?.removeEventListener("abort", cancel);
    uppy.destroy();
  }
};
