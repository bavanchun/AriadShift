"use client";

import type { ProfileId } from "@ariadshift/ariad-wasm";
import { AriadApiError, type Capabilities } from "@ariadshift/sdk";
import { Button } from "@ariadshift/ui/components/button";
import { CloudUploadIcon, UserIcon, WifiOffIcon } from "lucide-react";
import {
  type RefObject,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  type CloudApiOptions,
  cloudTargets,
  createCloudApi,
  formatFromName,
  type PlanLimits,
} from "@/lib/cloud/api";
import { type AccountUser, AuthError, CloudAuth } from "@/lib/cloud/auth";
import { CloudJob, type DiscardOutcome, type ServerCopy } from "@/lib/cloud/cloud-job";
import type { CloudConfig } from "@/lib/cloud/config";
import { type CloudAction, describeCloudError } from "@/lib/cloud/errors";
import {
  announcementFor,
  type CloudJobState,
  formatExpiry,
  formatRetention,
  INITIAL_JOB_STATE,
  isActive,
} from "@/lib/cloud/steps";
import type { Transfer } from "@/lib/cloud/transfer";
import { formatBytes } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";
import { PROFILES } from "@/lib/profiles";
import { AccountDialog } from "./account-dialog";
import { CloudJobRow } from "./cloud-job-row";
import { Turnstile } from "./turnstile";

type CloudProfile = Exclude<ProfileId, "private">;

export interface CloudPanelProps {
  config: CloudConfig;
  file: File;
  /** The format this device detected, if it could read the file. */
  sourceFormat: string | null;
  /** The target the page was opened for (`?to=`) or the local choice. */
  preferredTarget: string | null;
  profile: ProfileId;
  /** Switches the destination back to this device. */
  onUseDevice: () => void;
  /** Receives the function that deletes this file's server copies on "Start over". */
  cleanup: RefObject<(() => Promise<DiscardOutcome>) | null>;
  /** Told when leaving the panel could not delete the server copy, so it stays retryable. */
  onServerCopy: (copy: ServerCopy) => void;
  /** Told once a cloud upload or job has been created for this file. */
  onTransferStart?: () => void;
  /** Overrides for tests: the fetch and generated operations the API client uses. */
  apiOptions?: Pick<CloudApiOptions, "fetch" | "operations">;
  /** Overrides the Uppy transfer, for tests. */
  transfer?: Transfer;
}

const SELECT =
  "h-11 w-full rounded-lg border border-input bg-background px-3 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50";

const CLOUD_PROFILES = PROFILES.filter(
  (item): item is (typeof PROFILES)[number] & { id: CloudProfile } => item.id !== "private",
);

function useOnline(): boolean {
  return useSyncExternalStore(
    (notify) => {
      window.addEventListener("online", notify);
      window.addEventListener("offline", notify);
      return () => {
        window.removeEventListener("online", notify);
        window.removeEventListener("offline", notify);
      };
    },
    () => navigator.onLine,
    () => true,
  );
}

function limitsSummary(limits: PlanLimits): string {
  const max = limits.limits.max_input_bytes;
  const jobs = limits.quotas.jobs_per_day;
  const size = typeof max === "number" ? `files up to ${formatBytes(max)}` : "files of any size";
  const left =
    typeof jobs.remaining === "number" && typeof jobs.limit === "number"
      ? ` ${jobs.remaining} of ${jobs.limit} left today.`
      : "";
  if (!limits.authenticated) {
    const daily = typeof jobs.limit === "number" ? `${jobs.limit} cloud conversions a day, ` : "";
    return `Without an account: ${daily}${size}.${left}`;
  }
  return `${limits.plan} plan: ${size}.${left}`;
}

/** A reason the upload cannot start yet that the server's plan data already tells. */
function planRefusal(limits: PlanLimits | null, file: File): unknown {
  if (!limits) {
    return null;
  }
  const max = limits.limits.max_input_bytes;
  if (typeof max === "number" && file.size > max) {
    return new AriadApiError(413, "limit_exceeded", "the file is larger than the plan accepts");
  }
  const { quotas } = limits;
  const bytesLeft = quotas.input_bytes_per_day.remaining;
  if (
    quotas.jobs_per_day.remaining === 0 ||
    quotas.uploads_per_day.remaining === 0 ||
    quotas.pending_uploads.remaining === 0 ||
    (typeof bytesLeft === "number" && bytesLeft < file.size)
  ) {
    return new AriadApiError(429, "quota_exceeded", "the plan's allowance is used up");
  }
  return null;
}

export default function CloudPanel({
  config,
  file,
  sourceFormat,
  preferredTarget,
  profile: localProfile,
  onUseDevice,
  cleanup,
  onServerCopy,
  onTransferStart,
  apiOptions,
  transfer,
}: CloudPanelProps) {
  const testFetch = apiOptions?.fetch;
  const testOperations = apiOptions?.operations;
  const auth = useMemo(
    () => (config.authUrl ? new CloudAuth(config.authUrl, testFetch) : null),
    [config, testFetch],
  );
  const api = useMemo(
    () =>
      createCloudApi({
        baseUrl: config.apiUrl,
        bearer: auth?.bearer,
        fetch: testFetch,
        operations: testOperations,
      }),
    [auth, config, testFetch, testOperations],
  );
  const online = useOnline();

  const [user, setUser] = useState<AccountUser | null | undefined>(auth ? undefined : null);
  const [sessionExpired, setSessionExpired] = useState(false);
  const [capabilities, setCapabilities] = useState<Capabilities | null>(null);
  const [capabilitiesError, setCapabilitiesError] = useState<"api" | "auth" | null>(null);
  const [limits, setLimits] = useState<PlanLimits | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  const [profile, setProfile] = useState<CloudProfile>(
    localProfile === "private" ? "editable" : localProfile,
  );
  const [token, setToken] = useState<string | null>(null);
  const [turnstileKey, setTurnstileKey] = useState(0);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [signingOut, setSigningOut] = useState(false);
  const [signOutError, setSignOutError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [retryUntil, setRetryUntil] = useState(0);
  const [polite, setPolite] = useState("");
  const [assertive, setAssertive] = useState("");

  const statusRef = useRef<HTMLParagraphElement>(null);
  const errorRef = useRef<HTMLHeadingElement>(null);
  const tombstoneRef = useRef<HTMLParagraphElement>(null);
  const noticeRef = useRef<HTMLParagraphElement>(null);
  const accountRef = useRef<HTMLDivElement>(null);
  const headingId = useId();
  const blockedId = useId();
  const disclosureId = useId();

  const source =
    sourceFormat ?? (capabilities ? formatFromName(capabilities, file.name) : null) ?? null;
  const targets = useMemo(
    () => (capabilities && source ? cloudTargets(capabilities, source) : []),
    [capabilities, source],
  );

  // The latest values the job needs when it builds error copy.
  const context = useRef({ limits, user, source, target });
  context.current = { limits, user, source, target };

  const job = useMemo(
    () =>
      new CloudJob(file, {
        api,
        transfer,
        host: config.apiHost,
        owner: () =>
          context.current.user === undefined ? undefined : (context.current.user?.id ?? null),
        context: () => ({
          fileName: file.name,
          fileSize: file.size,
          from: context.current.source,
          to: context.current.target,
          limits: context.current.limits,
          signInOffered: auth !== null,
          signedIn: !!context.current.user,
        }),
        onUnauthorized: () => {
          auth?.forgetToken();
          void auth?.session().catch(() => undefined);
        },
      }),
    [api, auth, config.apiHost, file, transfer],
  );
  const onTransferStartRef = useRef(onTransferStart);
  onTransferStartRef.current = onTransferStart;
  const state = useSyncExternalStore(
    useCallback(
      (notify: () => void) =>
        job.subscribe((next) => {
          if (next.upload !== null || next.job !== null || next.phase === "uploading") {
            onTransferStartRef.current?.();
          }
          notify();
        }),
      [job],
    ),
    () => job.state,
    () => INITIAL_JOB_STATE,
  );

  // Only an explicit deletion removes a created job. Navigation and switching
  // destinations leave it available on /jobs. An upload without a job still
  // needs cleanup; hand a kept copy to the page so deletion remains retryable.
  const serverCopy = useRef(onServerCopy);
  serverCopy.current = onServerCopy;
  useEffect(() => {
    cleanup.current = () => job.discard();
    return () => {
      cleanup.current = null;
      if (!job.state.job) {
        void job.discard().then((outcome) => {
          if (outcome.status === "kept") {
            serverCopy.current(outcome);
          }
        });
      }
      job.dispose();
    };
  }, [cleanup, job]);

  useEffect(() => {
    if (!auth) {
      return;
    }
    const unsubscribe = auth.subscribe(() => {
      setUser(auth.user);
      setSessionExpired(auth.sessionExpired);
    });
    const onFocus = () => {
      if (auth.user) {
        void auth.session().catch(() => undefined);
      }
    };
    window.addEventListener("focus", onFocus);
    void auth.session().catch(() => undefined);
    return () => {
      window.removeEventListener("focus", onFocus);
      unsubscribe();
    };
  }, [auth]);

  useEffect(() => {
    if (capabilities || sessionExpired) {
      return;
    }
    const controller = new AbortController();
    api
      .capabilities(controller.signal)
      .then((value) => {
        setCapabilities(value);
        setCapabilitiesError(null);
      })
      .catch(
        (error) =>
          !controller.signal.aborted &&
          setCapabilitiesError(error instanceof AuthError ? "auth" : "api"),
      );
    return () => controller.abort();
  }, [api, capabilities, sessionExpired]);

  useEffect(() => {
    if (user === undefined || sessionExpired || !api.supportsLimits) {
      return;
    }
    const controller = new AbortController();
    api
      .limits(controller.signal)
      .then(setLimits)
      .catch(() => !controller.signal.aborted && setLimits(null));
    return () => controller.abort();
  }, [api, sessionExpired, user]);

  useEffect(() => {
    setTarget((current) => {
      if (current && targets.includes(current)) {
        return current;
      }
      if (preferredTarget && targets.includes(preferredTarget)) {
        return preferredTarget;
      }
      return targets[0] ?? null;
    });
  }, [preferredTarget, targets]);

  // One tick per second while a step's elapsed time shows, otherwise every
  // 30 seconds for the expiry line.
  const active = isActive(state.phase);
  useEffect(() => {
    const timer = setInterval(
      () => {
        setNow(Date.now());
        job.expireIfDue();
      },
      active || retryUntil > Date.now() ? 1000 : 30_000,
    );
    return () => clearInterval(timer);
  }, [active, job, retryUntil]);

  // Announce step changes and move focus where the user's next action is.
  const previous = useRef<CloudJobState>(INITIAL_JOB_STATE);
  useEffect(() => {
    const before = previous.current;
    previous.current = state;
    const message = announcementFor(before, state, file.name);
    if (message?.polite) {
      setPolite(message.polite);
    }
    if (message?.assertive) {
      setAssertive(message.assertive);
    }
    if (state.error && state.error !== before.error) {
      setRetryUntil(state.error.retryAfter ? Date.now() + state.error.retryAfter * 1000 : 0);
      queueMicrotask(() => errorRef.current?.focus());
    } else if (state.phase !== before.phase) {
      if (state.phase === "deleted" || state.phase === "canceled") {
        queueMicrotask(() => tombstoneRef.current?.focus());
      } else if (state.phase === "upload-canceled") {
        queueMicrotask(() => noticeRef.current?.focus());
      } else if (before.phase === "staged" || before.phase === "upload-canceled") {
        queueMicrotask(() => statusRef.current?.focus());
      }
    }
  }, [state, file.name]);

  const anonymous = user === null && !sessionExpired;
  const needsTurnstile = !!config.turnstileSiteKey && anonymous;
  const refusal = planRefusal(limits, file);
  const refusalView = refusal
    ? describeCloudError(refusal, {
        fileName: file.name,
        fileSize: file.size,
        from: source,
        to: target,
        limits,
        signInOffered: auth !== null,
        signedIn: !!user,
        serverExpiresAt: null,
      })
    : null;

  const blocked = !online
    ? "You are offline. Cloud conversion needs a connection; converting on this device still works."
    : sessionExpired
      ? "Your session has ended. Sign in again, or continue without an account."
      : user === undefined || (!capabilities && !capabilitiesError)
        ? "Connecting to the server…"
        : capabilitiesError === "auth"
          ? "The sign-in service is unavailable right now. Converting on this device still works."
          : capabilitiesError
            ? `${config.apiHost} cannot be reached right now. Converting on this device still works.`
            : !source
              ? `${config.apiHost} does not recognise this file type.`
              : targets.length === 0
                ? `${config.apiHost} cannot convert ${formatLabel(source)} files.`
                : !target
                  ? "Choose a target format first."
                  : refusalView
                    ? refusalView.message
                    : needsTurnstile && !token
                      ? "Complete the check that you are not a bot first."
                      : null;

  const consumeToken = () => {
    setToken(null);
    setTurnstileKey((key) => key + 1);
  };

  const continueAnonymously = () => {
    auth?.continueAnonymously();
    setUser(null);
    setSessionExpired(false);
    if (state.phase === "error" && !state.job) {
      job.reset();
    }
  };

  const uploadAndConvert = () => {
    if (blocked || !target) {
      return;
    }
    void job.start({ to: target, profile, turnstileToken: token ?? undefined });
    consumeToken();
  };

  const retry = () => {
    if (sessionExpired) {
      return;
    }
    const needsToken = job.retryNeedsUpload && needsTurnstile;
    if (needsToken && !token) {
      return;
    }
    void job.retry(needsToken ? (token ?? undefined) : undefined);
    if (needsToken) {
      consumeToken();
    }
  };

  const signOut = async () => {
    if (!auth || signingOut) {
      return;
    }
    setSigningOut(true);
    setSignOutError(null);
    try {
      await auth.signOut();
      setUser(null);
    } catch {
      const message = "Could not sign out right now. Try again.";
      setSignOutError(message);
      setAssertive(message);
    } finally {
      setSigningOut(false);
    }
  };

  const onAction = (action: Exclude<CloudAction, "retry" | "delete">) => {
    switch (action) {
      case "sign-in":
        setDialogOpen(true);
        return;
      case "use-anonymous":
        continueAnonymously();
        return;
      case "use-device":
        onUseDevice();
        return;
      case "turnstile":
        consumeToken();
        return;
      case "pick-target":
        job.reset();
        return;
    }
  };

  const composing = state.phase === "staged" || state.phase === "upload-canceled";
  const retryWait = Math.max(0, Math.ceil((retryUntil - now) / 1000));
  const retryNeedsToken = job.retryNeedsUpload && needsTurnstile;
  const retention = limits ? formatRetention(limits.retention_s) : null;
  const rowState =
    sessionExpired &&
    state.error &&
    !state.error.actions.includes("use-anonymous") &&
    (state.error.code === "unauthorized" || state.error.code === "session_expired")
      ? {
          ...state,
          error: {
            ...state.error,
            actions: [...state.error.actions, "use-anonymous", "use-device"] as CloudAction[],
          },
        }
      : state;

  return (
    <section
      aria-labelledby={headingId}
      className="flex flex-col gap-5 rounded-xl border border-border bg-card p-6 text-card-foreground"
      data-testid="cloud-panel"
    >
      <p className="sr-only" role="status" aria-live="polite" data-testid="cloud-announcer">
        {polite}
      </p>
      <p className="sr-only" aria-live="assertive" data-testid="cloud-alerts">
        {assertive}
      </p>

      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h2 id={headingId} className="text-lg font-semibold">
            Convert on {config.apiHost}
          </h2>
          <p className="text-sm text-muted-foreground" data-testid="cloud-limits">
            {limits
              ? limitsSummary(limits)
              : "The server's plan limits apply; a file it cannot take is refused."}
          </p>
        </div>
        {auth && (
          <div
            ref={accountRef}
            tabIndex={-1}
            className="flex flex-wrap items-center gap-2 text-sm outline-none"
            data-testid="cloud-account"
          >
            {user ? (
              <>
                <UserIcon aria-hidden="true" className="size-4" />
                <span>Signed in as {user.email || user.name}</span>
                <Button
                  variant="outline"
                  size="lg"
                  disabled={signingOut}
                  onClick={() => void signOut()}
                >
                  Sign out
                </Button>
              </>
            ) : user === null ? (
              <Button variant="outline" size="lg" onClick={() => setDialogOpen(true)}>
                Sign in
              </Button>
            ) : null}
          </div>
        )}
      </div>

      {signOutError && user && (
        <div
          role="alert"
          className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-destructive/40 p-3 text-sm"
          data-testid="cloud-sign-out-error"
        >
          <span>{signOutError}</span>
          <Button variant="outline" size="lg" disabled={signingOut} onClick={() => void signOut()}>
            Try again
          </Button>
        </div>
      )}

      {!online && (
        <p className="flex items-center gap-2 text-sm" data-testid="cloud-offline">
          <WifiOffIcon aria-hidden="true" className="size-4" />
          You are offline. Nothing can be uploaded until the connection is back.
        </p>
      )}

      {state.phase === "upload-canceled" && state.notice && (
        <p
          ref={noticeRef}
          tabIndex={-1}
          className="text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          data-testid="cloud-notice"
        >
          {state.notice}
        </p>
      )}

      {composing ? (
        <div className="flex flex-col gap-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-1.5">
              <label htmlFor={`${headingId}-target`} className="text-sm font-medium">
                Target format
              </label>
              <select
                id={`${headingId}-target`}
                aria-label={`Target format for ${file.name}`}
                className={SELECT}
                value={target ?? ""}
                onChange={(event) => setTarget(event.currentTarget.value || null)}
                disabled={targets.length === 0}
                data-testid="cloud-target"
              >
                {targets.length === 0 && <option value="">No targets</option>}
                {targets.map((id) => (
                  <option key={id} value={id}>
                    {formatLabel(id)}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-col gap-1.5">
              <label htmlFor={`${headingId}-profile`} className="text-sm font-medium">
                Profile
              </label>
              <select
                id={`${headingId}-profile`}
                aria-label={`Profile for ${file.name}`}
                className={SELECT}
                value={profile}
                onChange={(event) => setProfile(event.currentTarget.value as CloudProfile)}
              >
                {CLOUD_PROFILES.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div
            id={disclosureId}
            className="flex flex-col gap-2 rounded-lg border border-border bg-secondary p-4 text-sm text-secondary-foreground"
            data-testid="cloud-disclosure"
          >
            <p>
              This file will be uploaded to <strong>{config.apiHost}</strong> for conversion.{" "}
              {retention ? (
                <>
                  It is deleted automatically <strong>{retention}</strong> after upload, or sooner
                  when you choose Delete now.
                </>
              ) : (
                <>
                  It is deleted automatically when the server's retention period ends (the time
                  shows once it is uploaded), or sooner when you choose Delete now.
                </>
              )}
            </p>
            <p>
              The file name is stored with the job and never logged. Files are not used for anything
              other than this conversion.{" "}
              {config.operator ? `Operated by ${config.operator}. ` : ""}
              <a className="underline" href="/privacy">
                Privacy
              </a>
            </p>
          </div>

          {state.upload?.status === "completed" && state.expiresAt && (
            <div className="flex flex-wrap items-center gap-3 text-sm" data-testid="cloud-kept">
              <p>
                This file is already on {config.apiHost}. {formatExpiry(state.expiresAt, now)}.
              </p>
              <Button
                size="lg"
                variant="destructive"
                aria-label={`Delete ${file.name} from the server now`}
                onClick={() => void job.deleteNow()}
              >
                Delete now
              </Button>
            </div>
          )}

          {sessionExpired && (
            <div
              role="alert"
              className="flex flex-col gap-2 rounded-lg border border-destructive/40 p-3"
              data-testid="cloud-session-expired"
            >
              <p className="text-sm font-semibold text-destructive">Your session has ended</p>
              <p className="text-sm">
                Your session has ended. Sign in again, or continue without an account.
              </p>
              <div className="flex flex-wrap gap-2">
                <Button size="lg" variant="outline" onClick={() => setDialogOpen(true)}>
                  Sign in
                </Button>
                <Button size="lg" variant="outline" onClick={continueAnonymously}>
                  Continue without an account
                </Button>
                <Button size="lg" variant="outline" onClick={onUseDevice}>
                  Convert on this device
                </Button>
              </div>
            </div>
          )}

          {refusalView && !sessionExpired && (
            <div
              className="flex flex-col gap-2 rounded-lg border border-destructive/40 p-3"
              data-testid="cloud-refusal"
            >
              <p className="text-sm font-semibold text-destructive">{refusalView.title}</p>
              <p className="text-sm">{refusalView.message}</p>
              <div className="flex flex-wrap gap-2">
                {refusalView.actions.includes("sign-in") && (
                  <Button size="lg" variant="outline" onClick={() => setDialogOpen(true)}>
                    Sign in
                  </Button>
                )}
                <Button size="lg" variant="outline" onClick={onUseDevice}>
                  Convert on this device
                </Button>
              </div>
            </div>
          )}

          {needsTurnstile && config.turnstileSiteKey && !refusalView && (
            <Turnstile
              siteKey={config.turnstileSiteKey}
              onToken={setToken}
              resetKey={turnstileKey}
            />
          )}

          <div className="flex flex-col gap-2">
            <div>
              <Button
                size="lg"
                onClick={uploadAndConvert}
                aria-disabled={blocked ? true : undefined}
                aria-describedby={blocked ? blockedId : disclosureId}
                data-testid="cloud-start"
              >
                <CloudUploadIcon aria-hidden="true" />
                Upload and convert
              </Button>
            </div>
            {blocked && (
              <p
                id={blockedId}
                className="text-sm text-muted-foreground"
                data-testid="cloud-blocked"
              >
                {blocked}
              </p>
            )}
          </div>
        </div>
      ) : (
        <CloudJobRow
          state={rowState}
          fileName={file.name}
          host={config.apiHost}
          now={now}
          statusRef={statusRef}
          errorRef={errorRef}
          tombstoneRef={tombstoneRef}
          retryWait={retryWait}
          retryBlocked={
            !online
              ? "You are offline. Retry when the connection is back."
              : sessionExpired
                ? "Your session has ended. Sign in again, or continue without an account."
                : retryNeedsToken && !token
                  ? "Complete the check that you are not a bot first."
                  : null
          }
          retryExtra={
            retryNeedsToken && config.turnstileSiteKey && state.error?.actions.includes("retry") ? (
              <Turnstile
                siteKey={config.turnstileSiteKey}
                onToken={setToken}
                resetKey={turnstileKey}
              />
            ) : undefined
          }
          onCancel={() => void job.cancel()}
          onRetry={retry}
          onDelete={() => void job.deleteNow()}
          onDownload={(output, deleteAfter) => void job.download(output, deleteAfter)}
          onAction={onAction}
          onConvertAgain={() => job.reset()}
        />
      )}

      {state.job && config.authUrl && (
        <a href="/jobs" className="text-sm underline">
          View cloud jobs
        </a>
      )}

      {auth && dialogOpen && (
        <AccountDialog
          auth={auth}
          open={dialogOpen}
          onClose={() => setDialogOpen(false)}
          onSignedIn={(signedIn) => {
            if (auth.user?.id !== signedIn.id) {
              return;
            }
            setUser(signedIn);
            setSessionExpired(false);
            setSignOutError(null);
            setDialogOpen(false);
            if (state.phase === "error" && !state.job) {
              job.reset();
            }
            queueMicrotask(() => accountRef.current?.focus());
          }}
        />
      )}
    </section>
  );
}
