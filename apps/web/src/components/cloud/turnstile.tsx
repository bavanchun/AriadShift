"use client";

import { useEffect, useRef, useState } from "react";

// Cloudflare Turnstile for anonymous uploads, rendered explicitly so the script
// loads only when an anonymous cloud upload is about to happen. Tokens are
// single-use: the panel bumps `resetKey` after each upload request.

interface TurnstileApi {
  render(
    container: HTMLElement,
    options: {
      sitekey: string;
      action?: string;
      callback: (token: string) => void;
      "expired-callback": () => void;
      "error-callback": () => void;
    },
  ): string;
  reset(widgetId: string): void;
  remove(widgetId: string): void;
}

declare global {
  interface Window {
    turnstile?: TurnstileApi;
  }
}

const SCRIPT_URL = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

let loading: Promise<TurnstileApi> | null = null;

function loadTurnstile(): Promise<TurnstileApi> {
  if (window.turnstile) {
    return Promise.resolve(window.turnstile);
  }
  loading ??= new Promise<TurnstileApi>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = SCRIPT_URL;
    script.async = true;
    script.onload = () => {
      if (window.turnstile) {
        resolve(window.turnstile);
      } else {
        reject(new Error("Turnstile did not start"));
      }
    };
    script.onerror = () => {
      loading = null;
      script.remove();
      reject(new Error("Turnstile could not be loaded"));
    };
    document.head.append(script);
  });
  return loading;
}

export interface TurnstileProps {
  siteKey: string;
  /** Called with a fresh token, or `null` when the token expired or failed. */
  onToken: (token: string | null) => void;
  /** Changing it renders a new challenge. */
  resetKey: number;
}

export function Turnstile({ siteKey, onToken, resetKey }: TurnstileProps) {
  const container = useRef<HTMLDivElement>(null);
  const callback = useRef(onToken);
  callback.current = onToken;
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let widget: string | null = null;
    let api: TurnstileApi | null = null;
    let active = true;
    setProblem(null);
    callback.current(null);
    loadTurnstile()
      .then((loaded) => {
        if (!active || !container.current) {
          return;
        }
        api = loaded;
        widget = loaded.render(container.current, {
          sitekey: siteKey,
          action: "upload",
          callback: (token) => {
            setProblem(null);
            callback.current(token);
          },
          "expired-callback": () => callback.current(null),
          "error-callback": () => {
            callback.current(null);
            setProblem("The check could not run. Reload the page, or convert on this device.");
          },
        });
      })
      .catch(() => {
        if (active) {
          setProblem(
            "The bot check could not be loaded. Check your connection, or convert on this device.",
          );
        }
      });
    return () => {
      active = false;
      if (api && widget) {
        api.remove(widget);
      }
    };
  }, [siteKey, resetKey]);

  return (
    <div className="flex flex-col gap-1" data-testid="turnstile">
      <p className="text-sm text-muted-foreground">
        Uploads without an account need a quick check that you are not a bot.
      </p>
      <div ref={container} />
      {problem && (
        <p className="text-sm text-destructive" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}
