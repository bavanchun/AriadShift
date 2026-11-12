"use client";

import type * as React from "react";
import { type Platform, usePlatform } from "#hooks/use-platform";
import { cn } from "#lib/utils";

interface KeyLabel {
  mac: string;
  other: string;
  /** Spoken name, used when the visible glyph is a symbol. */
  name: string;
}

const KEYS: Record<string, KeyLabel> = {
  mod: { mac: "⌘", other: "Ctrl", name: "Command" },
  ctrl: { mac: "⌃", other: "Ctrl", name: "Control" },
  alt: { mac: "⌥", other: "Alt", name: "Option" },
  shift: { mac: "⇧", other: "Shift", name: "Shift" },
  enter: { mac: "↵", other: "Enter", name: "Enter" },
  backspace: { mac: "⌫", other: "Backspace", name: "Backspace" },
  delete: { mac: "⌦", other: "Del", name: "Delete" },
  esc: { mac: "⎋", other: "Esc", name: "Escape" },
};

/** Maps a key token to its visible glyph for a platform; unknown tokens are shown as written. */
export function keyLabel(token: string, platform: Platform): string {
  const known = KEYS[token.toLowerCase()];
  return known ? known[platform] : token;
}

function Kbd({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "inline-flex h-5 min-w-5 items-center justify-center rounded-sm border bg-muted px-1 font-sans text-[0.7rem] font-medium text-muted-foreground select-none",
        className,
      )}
      {...props}
    />
  );
}

export interface KbdGroupProps extends Omit<React.ComponentProps<"span">, "children"> {
  /**
   * Key tokens, for example `["mod", "K"]`. `mod`, `ctrl`, `alt`, `shift`, `enter`,
   * `backspace`, `delete` and `esc` adapt to the platform; anything else is shown as written.
   */
  keys: string[];
  /** Overrides detection, for server rendering and tests. */
  platform?: Platform;
}

/** A key combination such as ⌘K on macOS and Ctrl+K elsewhere. */
function KbdGroup({ keys, platform, className, ...props }: KbdGroupProps) {
  const detected = usePlatform();
  const target = platform ?? detected;
  return (
    <span
      data-slot="kbd-group"
      data-platform={target}
      // A key combination reads the same in every writing direction.
      dir="ltr"
      className={cn("inline-flex items-center gap-1", className)}
      {...props}
    >
      {keys.map((token, index) => {
        const known = KEYS[token.toLowerCase()];
        const glyph = keyLabel(token, target);
        const symbolic = target === "mac" && known !== undefined;
        return (
          // biome-ignore lint/suspicious/noArrayIndexKey: the same token may repeat, order is stable
          <span key={`${token}-${index}`} className="inline-flex items-center gap-1">
            {index > 0 && target === "other" && (
              <span className="text-xs text-muted-foreground">+</span>
            )}
            <Kbd>
              {symbolic ? (
                <>
                  <span aria-hidden="true">{glyph}</span>
                  <span className="sr-only">{known.name}</span>
                </>
              ) : (
                glyph
              )}
            </Kbd>
          </span>
        );
      })}
    </span>
  );
}

export { Kbd, KbdGroup };
