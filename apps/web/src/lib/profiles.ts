import type { ProfileId } from "@ariadshift/ariad-wasm";

/** Conversion goals; labels and descriptions are the `common.profile.<id>` messages. */
export const PROFILES: Array<{ id: ProfileId }> = [
  { id: "editable" },
  { id: "faithful" },
  { id: "fast" },
  { id: "private" },
];

export function isProfile(value: string | null): value is ProfileId {
  return PROFILES.some((profile) => profile.id === value);
}
