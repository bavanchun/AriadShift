"use client";
import { useCallback, useEffect, useState } from "react";

type LazyState<T> = { status: "idle" | "loading" | "failed" } | { status: "ready"; module: T };

/** Loads a module once `enabled` turns true, and lets the caller retry after a failure. */
export function useLazyModule<T>(load: () => Promise<T>, enabled: boolean) {
  const [state, setState] = useState<LazyState<T>>({ status: "idle" });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    setState((previous) => (previous.status === "ready" ? previous : { status: "loading" }));
    load().then(
      (module) => live && setState({ status: "ready", module }),
      () => live && setState({ status: "failed" }),
    );
    return () => {
      live = false;
    };
  }, [enabled, load, attempt]);
  const retry = useCallback(() => setAttempt((count) => count + 1), []);
  return { state, retry };
}
