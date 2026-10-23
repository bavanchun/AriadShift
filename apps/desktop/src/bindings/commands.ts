// Generated from Rust. Do not edit. Run just desktop-bindings.
import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";
export async function snapshot(request: T.Empty): Promise<T.Snapshot> {
  return invoke<T.Snapshot>("snapshot", { request });
}
export async function parse_link(request: T.LinkRequest): Promise<T.DeepLink | null> {
  return invoke<T.DeepLink | null>("parse_link", { request });
}
export async function enqueue(request: T.QueueRequest): Promise<Array<T.JobView>> {
  return invoke<Array<T.JobView>>("enqueue", { request });
}
export async function cancel(request: T.IdRequest): Promise<null> {
  return invoke<null>("cancel", { request });
}
export async function retry(request: T.IdRequest): Promise<Array<T.JobView>> {
  return invoke<Array<T.JobView>>("retry", { request });
}
export async function inspect(request: T.InspectRequest): Promise<T.Inspection> {
  return invoke<T.Inspection>("inspect", { request });
}
export async function settings(request: T.Settings): Promise<T.Settings> {
  return invoke<T.Settings>("settings", { request });
}
export async function watch(request: T.WatchRequest): Promise<T.WatchView> {
  return invoke<T.WatchView>("watch", { request });
}
export async function watch_control(request: T.WatchControl): Promise<null> {
  return invoke<null>("watch_control", { request });
}
export async function packs(request: T.Empty): Promise<Array<T.PackRow>> {
  return invoke<Array<T.PackRow>>("packs", { request });
}
export async function pack(request: T.PackRequest): Promise<string> {
  return invoke<string>("pack", { request });
}
export async function pack_cancel(request: T.Empty): Promise<null> {
  return invoke<null>("pack_cancel", { request });
}
export async function doctor(request: T.Empty): Promise<Array<T.Diagnostic>> {
  return invoke<Array<T.Diagnostic>>("doctor", { request });
}
