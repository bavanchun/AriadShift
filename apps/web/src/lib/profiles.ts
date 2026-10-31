import type { ProfileId } from "@ariadshift/ariad-wasm";

export const PROFILES: Array<{ id: ProfileId; label: string; description: string }> = [
  {
    id: "editable",
    label: "Editable",
    description: "Real headings, lists and tables you can keep editing.",
  },
  { id: "faithful", label: "Faithful", description: "Stay as close as possible to the layout." },
  { id: "fast", label: "Fast", description: "Prefer the quickest route." },
  { id: "private", label: "Private", description: "Only engines that never leave your device." },
];

export function isProfile(value: string | null): value is ProfileId {
  return PROFILES.some((profile) => profile.id === value);
}
