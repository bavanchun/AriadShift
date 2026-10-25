// One file's trip through the cloud: hash, reserve the upload, send it with
// Uppy, complete it, create the job, follow its events, download and delete.
// The controller owns the requests and the abort signal; the panel renders its
// state and calls its actions.
import type { AriadClient, Job, OutputFile, Upload } from "@ariadshift/sdk";
import { sha256Hex } from "@ariadshift/sdk";
import type { CloudApi } from "./api";
import {
  type CloudErrorContext,
  type CloudErrorView,
  describeCloudError,
  describeJobFailure,
} from "./errors";
import {
  applyJobEvent,
  type CloudJobState,
  formatClock,
  INITIAL_JOB_STATE,
  isActive,
  isRunning,
} from "./steps";
import type { Transfer } from "./transfer";

export interface StartOptions {
  to: string;
  profile?: "editable" | "faithful" | "fast";
  turnstileToken?: string;
}

export interface CloudJobDependencies {
  api: Pick<CloudApi, "client" | "deleteUpload" | "onEventConnection">;
  /** The API host named in confirmations, for example `api.example.com`. */
  host: string;
  /** Sends the bytes; Uppy's single presigned PUT by default. */
  transfer?: Transfer;
  /** Saves a downloaded blob under a file name; an `<a download>` click by default. */
  save?: (blob: Blob, filename: string) => void;
  /** Builds the error copy context for the current moment. */
  context: () => Omit<CloudErrorContext, "serverExpiresAt">;
  /** Called when the API refused the session, so the JWT is fetched again. */
  onUnauthorized?: () => void;
  now?: () => number;
}

type Client = Pick<
  AriadClient,
  | "createUpload"
  | "completeUpload"
  | "createJob"
  | "getJob"
  | "events"
  | "outputs"
  | "download"
  | "deleteJob"
>;

const loadUppyTransfer: Transfer = async (file, presigned, options) => {
  const { uppyTransfer } = await import("./transfer");
  return uppyTransfer(file, presigned, options);
};

/** Saves a blob through a temporary object URL and an `<a download>` click. */
export function saveBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.rel = "noopener";
  link.style.display = "none";
  document.body.append(link);
  link.click();
  link.remove();
  // Revoke later: some browsers read the URL after the click returns.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

function isAbort(error: unknown, signal: AbortSignal | undefined): boolean {
  return (
    signal?.aborted === true || (error instanceof DOMException && error.name === "AbortError")
  );
}

export class CloudJob {
  readonly #file: File;
  readonly #deps: CloudJobDependencies;
  readonly #client: Client;
  readonly #listeners = new Set<(state: CloudJobState) => void>();
  #state: CloudJobState = INITIAL_JOB_STATE;
  #controller: AbortController | null = null;
  #options: StartOptions | null = null;
  #lastEventId: number | undefined;
  #stopConnections: (() => void) | null = null;
  #discarded = false;

  constructor(file: File, deps: CloudJobDependencies) {
    this.#file = file;
    this.#deps = deps;
    this.#client = deps.api.client;
  }

  get state(): CloudJobState {
    return this.#state;
  }

  get file(): File {
    return this.#file;
  }

  subscribe(listener: (state: CloudJobState) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #now(): number {
    return this.#deps.now?.() ?? Date.now();
  }

  #set(patch: Partial<CloudJobState> | ((state: CloudJobState) => CloudJobState)) {
    this.#state = typeof patch === "function" ? patch(this.#state) : { ...this.#state, ...patch };
    for (const listener of this.#listeners) {
      listener(this.#state);
    }
  }

  #expiresAt(): string | null {
    return this.#state.job?.expires_at ?? this.#state.upload?.expires_at ?? null;
  }

  #describe(error: unknown): CloudErrorView {
    return describeCloudError(error, {
      ...this.#deps.context(),
      serverExpiresAt: this.#state.upload?.status === "completed" ? this.#expiresAt() : null,
    });
  }

  #fail(error: unknown) {
    const code = error instanceof Error && "code" in error ? error.code : null;
    if (code === "unauthorized") {
      this.#deps.onUnauthorized?.();
    }
    if (code === "expired") {
      // 410: the server already deleted the files, so there is nothing to retry against.
      this.#set({
        phase: "deleted",
        outputs: [],
        since: null,
        reconnecting: false,
        notice: `This file was already deleted from ${this.#deps.host}.`,
      });
      return;
    }
    this.#set({ phase: "error", error: this.#describe(error), reconnecting: false, since: null });
  }

  #signal(): AbortSignal {
    this.#controller?.abort();
    this.#controller = new AbortController();
    return this.#controller.signal;
  }

  /** Uploads the file and converts it. The file stays staged after any refusal. */
  async start(options: StartOptions): Promise<void> {
    this.#options = options;
    this.#discarded = false;
    const signal = this.#signal();
    const upload = this.#state.upload;
    // A completed upload can feed a new job; anything else starts over.
    if (upload?.status === "completed") {
      return this.#createJob(upload, signal);
    }
    this.#set({
      ...INITIAL_JOB_STATE,
      phase: "preparing",
      total: this.#file.size,
    });
    let created: Upload;
    try {
      const sha256 = await sha256Hex(this.#file);
      signal.throwIfAborted();
      created = await this.#client.createUpload(
        { filename: this.#file.name, size: this.#file.size, sha256 },
        { turnstileToken: options.turnstileToken, signal },
      );
    } catch (error) {
      if (!isAbort(error, signal)) {
        this.#fail(error);
      }
      return;
    }
    this.#set({ upload: created, expiresAt: created.expires_at, phase: "uploading", sent: 0 });

    try {
      if (!created.upload) {
        throw new Error("the upload has no presigned request");
      }
      await (this.#deps.transfer ?? loadUppyTransfer)(this.#file, created.upload, {
        signal,
        onProgress: (sent, total) => this.#set({ sent, total }),
      });
      this.#set({ phase: "checking", sent: this.#file.size });
      const completed = await this.#client.completeUpload(created.id, signal);
      this.#set({ upload: completed, expiresAt: completed.expires_at });
      await this.#createJob(completed, signal);
    } catch (error) {
      if (isAbort(error, signal)) {
        return;
      }
      this.#fail(error);
    }
  }

  async #createJob(upload: Upload, signal: AbortSignal) {
    const options = this.#options;
    if (!options) {
      return;
    }
    this.#set({ phase: "checking", error: null, failure: null, outputs: [], downloaded: [] });
    let job: Job;
    try {
      job = await this.#client.createJob({
        uploadId: upload.id,
        to: options.to,
        profile: options.profile,
        signal,
      });
    } catch (error) {
      if (!isAbort(error, signal)) {
        this.#fail(error);
      }
      return;
    }
    this.#lastEventId = undefined;
    this.#set((state) => ({
      ...state,
      job,
      expiresAt: job.expires_at,
      phase: "queued",
      since: this.#now(),
      warnings: job.warnings,
      attempt: job.attempts,
    }));
    await this.#follow(job.id, signal);
  }

  /** Follows the job's events; the SDK resumes dropped streams with `Last-Event-ID`. */
  async #follow(jobId: string, signal: AbortSignal) {
    this.#stopConnections?.();
    this.#stopConnections = this.#deps.api.onEventConnection((connection) => {
      if (connection.jobId !== jobId || signal.aborted) {
        return;
      }
      if (connection.attempt > 1 && connection.state !== "open") {
        this.#set({ reconnecting: true });
      }
    });
    try {
      for await (const event of this.#client.events(jobId, {
        signal,
        lastEventId: this.#lastEventId,
      })) {
        this.#lastEventId = event.id;
        this.#set((state) => applyJobEvent(state, event, this.#now()));
      }
      await this.#settle(jobId, signal);
    } catch (error) {
      if (isAbort(error, signal)) {
        return;
      }
      // The stream gave up; the job may still be fine on the server.
      try {
        await this.#settle(jobId, signal);
      } catch {
        this.#fail(error);
        return;
      }
      if (isRunning(this.#state.phase)) {
        this.#fail(error);
      }
    } finally {
      this.#stopConnections?.();
      this.#stopConnections = null;
    }
  }

  /** Reads the job once the stream ended, then the outputs of a succeeded job. */
  async #settle(jobId: string, signal: AbortSignal) {
    const job = await this.#client.getJob(jobId, signal);
    this.#set((state) => ({
      ...state,
      job,
      expiresAt: job.expires_at,
      warnings: job.warnings.length >= state.warnings.length ? job.warnings : state.warnings,
      reconnecting: false,
    }));
    switch (job.status) {
      case "succeeded": {
        const { outputs } = await this.#client.outputs(jobId, signal);
        this.#set({ phase: "succeeded", outputs, since: null });
        return;
      }
      case "failed": {
        const failure = job.error ?? this.#state.failure ?? { code: "failed", message: "" };
        this.#set({
          phase: "failed",
          since: null,
          failure,
          error: describeJobFailure(failure, {
            ...this.#deps.context(),
            serverExpiresAt: job.expires_at,
          }),
        });
        return;
      }
      case "canceled":
        this.#set({ phase: "canceled", since: null });
        return;
      case "expired":
        this.#set((state) => ({
          ...state,
          phase: state.phase === "deleted" ? "deleted" : "expired",
          since: null,
        }));
        return;
      default:
        return;
    }
  }

  /** Follows the events again after the connection gave up. */
  async resume(): Promise<void> {
    const job = this.#state.job;
    if (!job) {
      return;
    }
    const signal = this.#signal();
    this.#set({ phase: "queued", error: null, reconnecting: true, since: this.#now() });
    await this.#follow(job.id, signal);
  }

  /**
   * Retry after an error: resumes following a job that is still on the server,
   * or runs the step that failed again. A new upload needs a fresh Turnstile
   * token when the deployment requires one; tokens are single-use.
   */
  async retry(turnstileToken?: string): Promise<void> {
    const { job, phase, failure } = this.#state;
    if (job && phase === "error" && !failure && job.status !== "failed") {
      return this.resume();
    }
    if (this.#options) {
      return this.start({ ...this.#options, turnstileToken });
    }
  }

  /** Whether `retry` sends a new upload, which may need a Turnstile token. */
  get retryNeedsUpload(): boolean {
    return this.#state.upload?.status !== "completed";
  }

  /** Cancels the transfer, or the job while it is queued or running. */
  async cancel(): Promise<void> {
    const { phase, upload, job } = this.#state;
    if (isRunning(phase) && job) {
      await this.deleteNow();
      return;
    }
    if (!isActive(phase)) {
      return;
    }
    this.#controller?.abort();
    let notice = "Upload canceled. Nothing was sent.";
    if (upload) {
      let deleted = false;
      try {
        deleted = await this.#deps.api.deleteUpload(upload.id);
      } catch {
        deleted = false;
      }
      notice = deleted
        ? "Upload canceled. Nothing was kept."
        : `Upload canceled. Any partial data is deleted automatically at ${formatClock(upload.expires_at)}.`;
    }
    this.#set({
      ...INITIAL_JOB_STATE,
      phase: "upload-canceled",
      notice,
      upload: null,
    });
  }

  /** Deletes the job's input and outputs now (or the upload without a job). */
  async deleteNow(reason = "now"): Promise<void> {
    const { job, upload } = this.#state;
    const host = this.#deps.host;
    this.#controller?.abort();
    try {
      if (job) {
        await this.#client.deleteJob(job.id);
      } else if (upload) {
        const deleted = await this.#deps.api.deleteUpload(upload.id);
        if (!deleted) {
          this.#set({
            phase: "error",
            error: {
              ...this.#describe(new Error("upload deletion unavailable")),
              title: "Not deleted yet",
              message: `This server cannot delete an upload on request. It deletes the file automatically at ${formatClock(upload.expires_at)}.`,
              actions: [],
            },
          });
          return;
        }
      } else {
        return;
      }
    } catch (error) {
      const code = error instanceof Error && "code" in error ? error.code : null;
      // Already gone on the server: the outcome the user asked for.
      if (code !== "not_found" && code !== "expired") {
        this.#fail(error);
        return;
      }
    }
    this.#set({
      ...this.#state,
      phase: "deleted",
      since: null,
      reconnecting: false,
      error: null,
      outputs: [],
      notice: !job
        ? `Deleted from ${host}. Nothing was kept.`
        : reason === "download"
          ? `Deleted from ${host} after your download started. The input and output files are gone.`
          : `Deleted from ${host}. The input and output files are gone.`,
    });
  }

  /**
   * Downloads one output with fresh URLs (they last five minutes), saves it,
   * and deletes the job when `deleteAfter` is set and every output was saved.
   */
  async download(output: OutputFile, deleteAfter: boolean): Promise<void> {
    const job = this.#state.job;
    if (!job) {
      return;
    }
    try {
      const { outputs } = await this.#client.outputs(job.id);
      const fresh = outputs.find((candidate) => candidate.id === output.id) ?? output;
      const blob = await this.#client.download(fresh, { verifySha256: true });
      (this.#deps.save ?? saveBlob)(blob, fresh.filename);
    } catch (error) {
      this.#fail(error);
      return;
    }
    const downloaded = [...new Set([...this.#state.downloaded, output.id])];
    this.#set({ downloaded });
    if (
      deleteAfter &&
      this.#state.outputs.every((candidate) => downloaded.includes(candidate.id))
    ) {
      await this.deleteNow("download");
    }
  }

  /** Marks the job expired once its retention passed, without a request. */
  expireIfDue() {
    const expiresAt = this.#expiresAt();
    if (
      expiresAt &&
      new Date(expiresAt).getTime() <= this.#now() &&
      (this.#state.phase === "succeeded" || this.#state.phase === "failed")
    ) {
      this.#set({ phase: "expired", outputs: [], error: null });
    }
  }

  /**
   * Deletes whatever this file left on the server, for "Start over and delete
   * files" and for leaving cloud mode. Returns whether anything was deleted.
   */
  async discard(): Promise<boolean> {
    const { job, upload, phase } = this.#state;
    this.#controller?.abort();
    if (this.#discarded || (!job && !upload) || phase === "deleted" || phase === "expired") {
      return false;
    }
    this.#discarded = true;
    try {
      if (job) {
        await this.#client.deleteJob(job.id);
        return true;
      }
      if (upload) {
        return await this.#deps.api.deleteUpload(upload.id);
      }
    } catch {
      // The retention period still deletes it.
    }
    return false;
  }

  /**
   * Back to choosing a target. An upload the server verified is kept for the
   * next job, unless it was deleted with its job.
   */
  reset() {
    this.#controller?.abort();
    const { upload, phase, job } = this.#state;
    const keep = phase === "error" && !job && upload?.status === "completed" ? upload : null;
    this.#set({ ...INITIAL_JOB_STATE, upload: keep, expiresAt: keep?.expires_at ?? null });
  }

  /** Stops every request without touching the server. */
  dispose() {
    this.#controller?.abort();
    this.#stopConnections?.();
    this.#listeners.clear();
  }
}
