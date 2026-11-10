"use client";

import { useSyncExternalStore } from "react";

export type Platform = "mac" | "other";

const subscribe = () => () => {};

function detect(): Platform {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const name = nav.userAgentData?.platform ?? nav.platform ?? "";
  return /mac|iphone|ipad|ipod/i.test(name) ? "mac" : "other";
}

/** The keyboard platform family. Server and hydration render as `other`, then the browser value applies. */
export function usePlatform(): Platform {
  return useSyncExternalStore(subscribe, detect, () => "other");
}
