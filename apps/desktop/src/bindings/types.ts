// Generated from Rust. Do not edit. Run just desktop-bindings.
export type Empty = Record<symbol, never>;
export type Profile = "editable" | "faithful" | "fast" | "private";
export type OutputPolicy = "adjacent" | "folder";
export type Settings = {
  profile: Profile;
  output_policy: OutputPolicy;
  output_folder: string | null;
  overwrite: boolean;
  telemetry: boolean;
  concurrency: number;
};
export type QueueRequest = {
  paths: Array<string>;
  target: string;
  profile: Profile;
  output_folder: string | null;
  overwrite: boolean;
};
export type IdRequest = { id: string };
export type InspectRequest = { path: string; target: string; profile: Profile };
export type Inspection = {
  summary: string;
  targets: Array<string>;
  explanation: string;
  warnings: Array<string>;
};
export type JobStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type JobView = {
  id: string;
  input: string;
  output: string;
  target: string;
  profile: Profile;
  status: JobStatus;
  stage: string;
  error: string | null;
  warnings: Array<string>;
};
export type WatchRequest = {
  interval_seconds: number;
  recursive: boolean;
  input: string;
  output: string;
  from: string;
  target: string;
  profile: Profile;
  overwrite: boolean;
};
export type WatchView = { id: string; route: WatchRequest; paused: boolean; error: string | null };
export type WatchControl = { id: string; paused: boolean; remove: boolean };
export type PackRow = {
  name: string;
  version: string;
  description: string;
  installed: boolean;
  active: boolean;
};
export type PackAction = "install" | "verify" | "remove";
export type PackRequest = {
  name: string;
  version: string | null;
  action: PackAction;
  from: string | null;
};
export type Diagnostic = {
  id: string;
  ok: boolean;
  required: boolean;
  message: string;
  hint: string;
};
export type Snapshot = {
  jobs: Array<JobView>;
  watches: Array<WatchView>;
  settings: Settings;
  pack_progress: string;
  pack_busy: boolean;
};
export type DeepLink = { from: string; target: string; profile: Profile };
export type LinkRequest = { url: string };
