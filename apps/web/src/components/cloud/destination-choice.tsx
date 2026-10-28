"use client";

import { CloudIcon, ShieldCheckIcon } from "lucide-react";
import { useId } from "react";

export type Destination = "device" | "cloud";

export interface DestinationChoiceProps {
  value: Destination;
  onChange: (value: Destination) => void;
  /** The API host the cloud option names, for example `api.example.com`. */
  host: string;
  disabled?: boolean;
}

const OPTION =
  "flex min-h-11 cursor-pointer items-start gap-3 rounded-lg border border-border bg-card p-3 text-sm has-checked:border-primary has-checked:bg-secondary has-focus-visible:ring-3 has-focus-visible:ring-ring/50";

/** The explicit choice of where this file is converted; the device is the default. */
export function DestinationChoice({ value, onChange, host, disabled }: DestinationChoiceProps) {
  const name = useId();
  return (
    <fieldset className="m-0 flex min-w-0 flex-col gap-2 border-0 p-0" data-testid="destination">
      <legend className="mb-2 text-sm font-semibold">Where to convert</legend>
      <div className="grid gap-2 sm:grid-cols-2">
        <label className={OPTION}>
          <input
            type="radio"
            name={name}
            value="device"
            checked={value === "device"}
            disabled={disabled}
            onChange={() => onChange("device")}
            className="mt-0.5 size-4 accent-primary"
          />
          <span className="flex flex-col gap-0.5">
            <span className="flex items-center gap-1.5 font-semibold">
              <ShieldCheckIcon aria-hidden="true" className="size-4" />
              On this device
            </span>
            <span className="text-muted-foreground">Nothing is uploaded.</span>
          </span>
        </label>
        <label className={OPTION}>
          <input
            type="radio"
            name={name}
            value="cloud"
            checked={value === "cloud"}
            disabled={disabled}
            onChange={() => onChange("cloud")}
            className="mt-0.5 size-4 accent-primary"
          />
          <span className="flex flex-col gap-0.5">
            <span className="flex items-center gap-1.5 font-semibold">
              <CloudIcon aria-hidden="true" className="size-4" />
              On {host}
            </span>
            <span className="text-muted-foreground">
              Uploads this file only when you choose Upload and convert.
            </span>
          </span>
        </label>
      </div>
    </fieldset>
  );
}
