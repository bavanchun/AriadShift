// The `/v1` calls the jobs page makes, through the SDK. A signed-in user's
// requests carry a JWT from the auth service; anonymous jobs are read without
// credentials, since a bearer token would look for them in the wrong workspace.

import { type AriadClient, createAriadClient } from "@ariadshift/sdk";
import type { AuthApi } from "./auth-api";

export type JobsApi = Pick<
  AriadClient,
  "getJob" | "events" | "outputs" | "download" | "deleteJob" | "createJob"
>;

export function createJobsApi(apiUrl: string, auth: AuthApi, signedIn: boolean): JobsApi {
  return createAriadClient({
    baseUrl: apiUrl,
    ...(signedIn ? { credentials: { bearer: () => auth.apiToken() } } : {}),
  });
}

/** Saves a downloaded file through a temporary object URL. */
export function saveBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // Revoke after the browser has started reading the URL.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
