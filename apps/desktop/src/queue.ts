import type { JobStatus, JobView } from "./bindings/types";

export const isActive = (status: JobStatus) => status === "queued" || status === "running";

export type QueueSummary = Record<JobStatus, number> & {
  /** Queued and running jobs. */
  active: number;
  /** Jobs added since the queue was last idle, and how many of them have finished. */
  batchTotal: number;
  batchDone: number;
};

/**
 * Counts jobs by status. `settled` holds the jobs that had finished when the queue was last
 * idle, so the batch progress covers only work started since then.
 */
export function summarize(jobs: JobView[], settled: ReadonlySet<string>): QueueSummary {
  const summary: QueueSummary = {
    queued: 0,
    running: 0,
    succeeded: 0,
    failed: 0,
    cancelled: 0,
    active: 0,
    batchTotal: 0,
    batchDone: 0,
  };
  for (const job of jobs) {
    summary[job.status] += 1;
    if (isActive(job.status)) summary.active += 1;
    if (settled.has(job.id)) continue;
    summary.batchTotal += 1;
    if (!isActive(job.status)) summary.batchDone += 1;
  }
  return summary;
}

/** The settled set for the next snapshot: every job once the queue is idle, else unchanged. */
export const nextSettled = (jobs: JobView[], settled: ReadonlySet<string>) =>
  jobs.some((job) => isActive(job.status)) ? settled : new Set(jobs.map((job) => job.id));

/**
 * Jobs that finished since the previous snapshot: they were queued or running, or they
 * appeared and finished between two snapshots. `previous` is `undefined` for the first
 * snapshot, whose finished jobs are history rather than news.
 */
export function newlyFinished(
  previous: ReadonlyMap<string, JobStatus> | undefined,
  jobs: JobView[],
) {
  if (!previous) return [];
  return jobs.filter((job) => {
    if (isActive(job.status)) return false;
    const before = previous.get(job.id);
    return before === undefined || isActive(before);
  });
}

export const statusMap = (jobs: JobView[]) =>
  new Map(jobs.map((job) => [job.id, job.status] as const));

const rank: Record<JobStatus, number> = {
  failed: 0,
  running: 1,
  queued: 2,
  succeeded: 3,
  cancelled: 3,
};

/** Failures first, then running, waiting and finished work; newest first within each group. */
export const orderJobs = (jobs: JobView[]) =>
  jobs.toReversed().sort((a, b) => rank[a.status] - rank[b.status]);
