import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { ThemeToggle } from "@ariadshift/ui/components/theme-toggle";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { check } from "@tauri-apps/plugin-updater";
import { UploadIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import * as commands from "./bindings/commands";
import type {
  Diagnostic,
  Inspection,
  JobStatus,
  PackAction,
  PackRow,
  Profile,
  Settings,
  Snapshot,
} from "./bindings/types";
import { finishedToasts, Kbd, StatusBadge, type Toast, Toasts } from "./feedback";
import { formats, inputExtensions, fileName as name, partitionPaths } from "./formats";
import {
  isActive,
  newlyFinished,
  nextSettled,
  orderJobs,
  type QueueSummary,
  statusMap,
  summarize,
} from "./queue";
import { ariaShortcut, matchShortcut, shortcutLabel, shortcutList } from "./shortcuts";

const profiles: Profile[] = ["editable", "faithful", "fast", "private"];
const tabs = [
  "Convert",
  "Queue",
  "Watch folders",
  "Engine packs",
  "Diagnostics",
  "Settings",
] as const;
type Tab = (typeof tabs)[number];
// The native `file_open::EVENT`: the OS file manager opened files with the app.
const openFilesEvent = "open-files";
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;

/** Info notices confirm an action and leave on their own; warnings and errors wait for the user. */
type Notice = { tone: "info" | "warning" | "error"; text: string };
export const noticeTimeout = 6000;
const failure = (error: unknown): Notice => ({ tone: "error", text: message(error) });
const info = (text: string): Notice => ({ tone: "info", text });

function skippedNotice(skipped: string[]): Notice {
  const names = skipped.slice(0, 5).map(name).join(", ");
  const more = skipped.length > 5 ? ` and ${skipped.length - 5} more` : "";
  return {
    tone: "warning",
    text: `Skipped ${plural(skipped.length, "item")} that ${skipped.length === 1 ? "is not a supported document" : "are not supported documents"}: ${names}${more}.`,
  };
}

function queueText(summary: QueueSummary) {
  const parts: string[] = [];
  if (summary.running) parts.push(`${summary.running} converting`);
  if (summary.queued) parts.push(`${summary.queued} waiting`);
  if (summary.active) {
    parts.push(`${summary.batchDone} of ${summary.batchTotal} finished`);
    return parts.join(" · ");
  }
  if (summary.succeeded) parts.push(`${summary.succeeded} converted`);
  if (summary.failed) parts.push(`${summary.failed} failed`);
  if (summary.cancelled) parts.push(`${summary.cancelled} cancelled`);
  return parts.length ? `All done · ${parts.join(" · ")}` : "";
}

function diagnosticsText(diagnostics: Diagnostic[]) {
  const failing = diagnostics.filter((d) => !d.ok);
  const required = failing.filter((d) => d.required).length;
  if (!failing.length) return `All ${plural(diagnostics.length, "check")} passed.`;
  return `${plural(failing.length, "check")} ${failing.length === 1 ? "needs" : "need"} attention${required ? `, ${required} required` : ""}. Each hint below says how to fix it.`;
}

type Dragging = { supported: string[]; skipped: string[] };

function DropOverlay({ dragging }: { dragging: Dragging }) {
  const { supported, skipped } = dragging;
  const unknown = !supported.length && !skipped.length;
  const title = unknown
    ? "Drop to add documents"
    : supported.length
      ? `Drop to add ${plural(supported.length, "document")}`
      : "These items can’t be converted";
  const detail = supported.length
    ? skipped.length
      ? `${plural(skipped.length, "item")} will be skipped.`
      : "They join the Convert selection."
    : unknown
      ? "They join the Convert selection."
      : "Drop Markdown, HTML, Office, EPUB, PDF or image files.";
  return (
    <div
      className="drop-overlay"
      data-accepts={supported.length > 0 || unknown || undefined}
      aria-hidden="true"
    >
      <div className="drop-overlay-card">
        <UploadIcon />
        <strong>{title}</strong>
        <p>{detail}</p>
      </div>
    </div>
  );
}

export function App() {
  const [tab, setTab] = useState<Tab>("Convert");
  const [state, setState] = useState<Snapshot>();
  const [settings, setSettings] = useState<Settings>();
  const [savedSettings, setSavedSettings] = useState<Settings>();
  const initialized = useRef(false);
  const [notice, setNotice] = useState<Notice>();
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dragging, setDragging] = useState<Dragging>();
  const [settled, setSettled] = useState<ReadonlySet<string>>(() => new Set());
  const statuses = useRef<Map<string, JobStatus> | undefined>(undefined);
  const currentTab = useRef(tab);
  const heading = useRef<HTMLHeadingElement>(null);
  const [paths, setPaths] = useState<string[]>([]);
  const [typedPath, setTypedPath] = useState("");
  const [target, setTarget] = useState("html");
  const [profile, setProfile] = useState<Profile>("editable");
  const [source, setSource] = useState("");
  const [output, setOutput] = useState<string | null>(null);
  const [overwrite, setOverwrite] = useState(false);
  const [inspection, setInspection] = useState<Inspection>();
  const [working, setWorking] = useState(false);
  const [watchInput, setWatchInput] = useState("");
  const [watchOutput, setWatchOutput] = useState("");
  const [watchFrom, setWatchFrom] = useState("md");
  const [watchTarget, setWatchTarget] = useState("html");
  const [watchProfile, setWatchProfile] = useState<Profile>("editable");
  const [watchOverwrite, setWatchOverwrite] = useState(false);
  const [watchInterval, setWatchInterval] = useState(2);
  const [watchRecursive, setWatchRecursive] = useState(false);
  const [packs, setPacks] = useState<PackRow[]>();
  const [packName, setPackName] = useState("docling");
  const [packFrom, setPackFrom] = useState("");
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>();
  const [checking, setChecking] = useState(false);
  const [update, setUpdate] = useState<Awaited<ReturnType<typeof check>>>(null);
  const [updateProgress, setUpdateProgress] = useState("");

  const dismissToast = useCallback(
    (id: string) => setToasts((current) => current.filter((toast) => toast.id !== id)),
    [],
  );
  const refresh = useCallback(async () => {
    const next = await commands.snapshot({});
    const finished = newlyFinished(statuses.current, next.jobs).filter(
      (job) => job.status !== "cancelled",
    );
    statuses.current = statusMap(next.jobs);
    setSettled((previous) => nextSettled(next.jobs, previous));
    setState(next);
    // The Queue tab already shows each result, so toasts only reach users looking elsewhere.
    if (finished.length && currentTab.current !== "Queue") {
      const created = finishedToasts(finished, {
        reveal: (job) => void revealItemInDir(job.output).catch((e) => setNotice(failure(e))),
        retry: (job) => void commands.retry({ id: job.id }).catch((e) => setNotice(failure(e))),
        viewQueue: () => setTab("Queue"),
      });
      const ids = new Set(created.map((toast) => toast.id));
      setToasts((current) => [...current.filter((t) => !ids.has(t.id)), ...created].slice(-3));
    }
    if (!initialized.current) {
      initialized.current = true;
      setSettings(next.settings);
      setSavedSettings(next.settings);
      setProfile(next.settings.profile);
      setWatchProfile(next.settings.profile);
      setWatchOverwrite(next.settings.overwrite);
      setOverwrite(next.settings.overwrite);
      if (next.settings.output_policy === "folder") setOutput(next.settings.output_folder);
    }
  }, []);
  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      setNotice(undefined);
      try {
        await action();
        await refresh();
      } catch (error) {
        setNotice(failure(error));
      }
    },
    [refresh],
  );
  const addPaths = useCallback((incoming: string[]) => {
    setPaths((current) => [...new Set([...current, ...incoming])]);
    setInspection(undefined);
    setTab("Convert");
  }, []);
  const addDropped = useCallback(
    (dropped: string[]) => {
      const { supported, skipped } = partitionPaths(dropped);
      if (supported.length) addPaths(supported);
      if (skipped.length) setNotice(skippedNotice(skipped));
    },
    [addPaths],
  );
  const pick = useCallback(
    async (from?: string) => {
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "Documents",
            extensions: from
              ? (inputExtensions[from] ?? [from])
              : Object.values(inputExtensions).flat(),
          },
        ],
      });
      if (selected) addPaths(Array.isArray(selected) ? selected : [selected]);
    },
    [addPaths],
  );
  // A failed listing still ends the loading state, so the empty state can offer a reload.
  const loadPacks = useCallback(async () => {
    try {
      setPacks(await commands.packs({}));
    } catch (error) {
      setPacks((current) => current ?? []);
      throw error;
    }
  }, []);
  const chooseFolder = async (setter: (folder: string) => void) => {
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder === "string") setter(folder);
  };
  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        if (active) await refresh();
      } catch (e) {
        if (active) setNotice(failure(e));
      }
    };
    void poll();
    const timer = setInterval(poll, 500);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [refresh]);
  useEffect(() => {
    let disposed = false;
    const listeners: (() => void)[] = [];
    const keep = (stop: () => void) => (disposed ? stop() : listeners.push(stop));
    let last = "";
    let lastAt = 0;
    const links = async (urls: string[]) => {
      for (const url of urls) {
        if (url === last && Date.now() - lastAt < 1000) continue;
        last = url;
        lastAt = Date.now();
        try {
          const link = await commands.parse_link({ url });
          if (!link || disposed) continue;
          setTarget(link.target);
          setSource(link.from);
          setProfile(link.profile);
          setTab("Convert");
          setNotice(
            info(
              `Desktop link: ${link.from} → ${link.target} · ${link.profile}. Choose a local file.`,
            ),
          );
          await pick(link.from);
        } catch (e) {
          if (!disposed) setNotice(failure(e));
        }
      }
    };
    void onOpenUrl((urls) => {
      void links(urls);
    })
      .then(keep)
      .catch((e) => setNotice(failure(e)));
    void getCurrent()
      .then((urls) => urls && links(urls))
      .catch((e) => setNotice(failure(e)));
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const drag = event.payload;
        if (drag.type === "enter") setDragging(partitionPaths(drag.paths));
        else if (drag.type === "leave") setDragging(undefined);
        else if (drag.type === "drop") {
          setDragging(undefined);
          addDropped(drag.paths);
        }
      })
      .then(keep)
      .catch((e) => setNotice(failure(e)));
    // Draining removes the files from the native list, so they are added even after disposal:
    // a StrictMode remount keeps this component's state.
    const opened = () =>
      commands
        .opened_files({})
        .then((files) => {
          if (files.length > 0) addPaths(files);
        })
        .catch((e) => setNotice(failure(e)));
    // Subscribe first, then drain once: files opened before the subscription are still in the
    // native list, and files opened after it raise the event.
    void listen(openFilesEvent, () => void opened())
      .then(keep)
      .catch((e) => setNotice(failure(e)))
      .finally(() => void opened());
    return () => {
      disposed = true;
      for (const stop of listeners) stop();
    };
  }, [pick, addPaths, addDropped]);
  useEffect(() => {
    if (!paths[0]) {
      setInspection(undefined);
      return;
    }
    let active = true;
    void commands
      .inspect({ path: paths[0], target, profile })
      .then((result) => {
        if (active) setInspection(result);
      })
      .catch((e) => {
        if (active)
          setInspection({
            summary: "Cannot inspect this file",
            targets: [],
            explanation: message(e),
            warnings: [],
          });
      });
    return () => {
      active = false;
    };
  }, [paths, target, profile]);
  useEffect(() => {
    if (tab === "Engine packs") void run(loadPacks);
  }, [tab, run, loadPacks]);
  // Moving focus to the page heading announces the new tab, as a route change would.
  useEffect(() => {
    if (currentTab.current === tab) return;
    currentTab.current = tab;
    heading.current?.focus();
  }, [tab]);
  useEffect(() => {
    if (notice?.tone !== "info") return;
    const timer = setTimeout(() => setNotice(undefined), noticeTimeout);
    return () => clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    let active = true;
    void Promise.resolve(check())
      .then((found) => {
        if (active && found) {
          setUpdate(found);
          setUpdateProgress(`Version ${found.version} is available`);
        }
      })
      .catch(() => {
        // Offline startup must leave local conversion available; explicit checks show errors.
      });
    return () => {
      active = false;
    };
  }, []);

  const canConvert = paths.length > 0 && !working;
  const start = async () => {
    setWorking(true);
    await run(async () => {
      await commands.enqueue({ paths, target, profile, output_folder: output, overwrite });
      setPaths([]);
      setInspection(undefined);
      setTab("Queue");
    });
    setWorking(false);
  };
  const chooseFiles = () => void run(() => pick(source || undefined));
  const jobs = state?.jobs ?? [];
  const summary = summarize(jobs, settled);
  const cancelPending = () =>
    void run(async () => {
      for (const j of jobs) if (isActive(j.status)) await commands.cancel({ id: j.id });
    });
  const copyError = (error: string) =>
    void navigator.clipboard
      .writeText(error)
      .then(() => setNotice(info("Error copied")))
      .catch((e) => setNotice(failure(e)));

  // The listener is installed once; the handler it calls is replaced after every render so it
  // always sees the current selection and messages.
  const onShortcut = useRef<((event: KeyboardEvent) => void) | undefined>(undefined);
  useEffect(() => {
    onShortcut.current = (event) => {
      const action = matchShortcut(event, tabs.length);
      if (!action) return;
      if (action.kind === "tab") {
        event.preventDefault();
        setTab(tabs[action.index] ?? "Convert");
      } else if (action.kind === "settings") {
        event.preventDefault();
        setTab("Settings");
      } else if (action.kind === "open") {
        event.preventDefault();
        setTab("Convert");
        chooseFiles();
      } else if (action.kind === "convert") {
        if (tab !== "Convert" || !canConvert) return;
        event.preventDefault();
        void start();
      } else if (action.kind === "cancel") {
        if (tab !== "Queue" || !summary.active) return;
        event.preventDefault();
        cancelPending();
      } else if (!event.defaultPrevented) {
        const latest = toasts.at(-1);
        if (notice) setNotice(undefined);
        else if (latest) dismissToast(latest.id);
      }
    };
  });
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => onShortcut.current?.(event);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const packAction = async (action: PackAction, pack: { name: string; version?: string }) => {
    await run(async () => {
      const result = await commands.pack({
        name: pack.name,
        version: pack.version ?? null,
        action,
        from: action === "install" && packFrom ? packFrom : null,
      });
      setNotice(info(result));
      setPacks(await commands.packs({}));
    });
  };
  const selectProfile = (value: Profile, setter: (value: Profile) => void, id: string) => (
    <label htmlFor={id}>
      Profile
      <select id={id} value={value} onChange={(e) => setter(e.target.value as Profile)}>
        {profiles.map((p) => (
          <option key={p}>{p}</option>
        ))}
      </select>
    </label>
  );
  const settingsChanged =
    settings !== undefined && JSON.stringify(settings) !== JSON.stringify(savedSettings);

  return (
    <div className="desktop-shell">
      <aside>
        <div className="brand">
          <img src="/icon.png" width="40" height="40" alt="" />
          <div>
            AriadShift<small>Documents, connected.</small>
          </div>
        </div>
        <nav aria-label="Main navigation">
          {tabs.map((item, index) => (
            <button
              type="button"
              aria-current={tab === item ? "page" : undefined}
              aria-keyshortcuts={ariaShortcut(String(index + 1))}
              key={item}
              onClick={() => setTab(item)}
            >
              <span>
                {item}
                {item === "Queue" && state ? ` (${summary.active})` : ""}
              </span>
              <Kbd>{shortcutLabel(String(index + 1))}</Kbd>
            </button>
          ))}
        </nav>
        <div className="local-note">
          <Badge>Local processing</Badge>
          <p>Your documents stay on this computer.</p>
          <ThemeToggle />
        </div>
      </aside>
      <main>
        <header>
          <p className="eyebrow">YOUR LOCAL WORKSPACE</p>
          <h1 ref={heading} tabIndex={-1}>
            {tab}
          </h1>
          <p>
            {tab === "Convert"
              ? "Turn documents into the format you need. Keep the thread of your work."
              : "Private tools for your document workflow."}
          </p>
        </header>
        {notice && (
          <div
            role={notice.tone === "error" ? "alert" : "status"}
            className="notice"
            data-tone={notice.tone}
          >
            {notice.text}
            <button
              type="button"
              aria-label="Dismiss notice"
              aria-keyshortcuts="Escape"
              onClick={() => setNotice(undefined)}
            >
              ×
            </button>
          </div>
        )}
        {update && tab !== "Settings" && (
          <div role="status" className="notice">
            Version {update.version} is available.
            <Button variant="outline" onClick={() => setTab("Settings")}>
              View update
            </Button>
          </div>
        )}
        {!state && <p role="status">Connecting to the desktop engine…</p>}
        {tab === "Convert" && (
          <>
            <section
              className="drop-panel"
              aria-label="Drop documents"
              data-dragging={dragging ? true : undefined}
            >
              <div className="drop-symbol">↳</div>
              <h2>Drop your documents here</h2>
              <p>Markdown, HTML, Office, EPUB, PDF and images · Single files or a batch</p>
              <Button size="lg" aria-keyshortcuts={ariaShortcut("o")} onClick={chooseFiles}>
                Choose files <Kbd>{shortcutLabel("o")}</Kbd>
              </Button>
              <details>
                <summary>Add a local file by path</summary>
                <div className="row">
                  <label htmlFor="file-path">
                    File path
                    <input
                      id="file-path"
                      value={typedPath}
                      onChange={(e) => setTypedPath(e.target.value)}
                      placeholder="Absolute path to a document"
                    />
                  </label>
                  <Button
                    variant="outline"
                    disabled={!typedPath}
                    onClick={() => {
                      addPaths([typedPath]);
                      setTypedPath("");
                    }}
                  >
                    Add file
                  </Button>
                </div>
              </details>
            </section>
            {paths.length > 0 && (
              <section className="panel">
                <h2>
                  {paths.length} selected {paths.length === 1 ? "file" : "files"}
                </h2>
                <ul className="file-list">
                  {paths.map((p) => (
                    <li key={p}>
                      <span title={p}>{name(p)}</span>
                      <Button
                        variant="ghost"
                        aria-label={`Remove ${name(p)}`}
                        onClick={() => setPaths((all) => all.filter((f) => f !== p))}
                      >
                        Remove
                      </Button>
                    </li>
                  ))}
                </ul>
                <Button variant="outline" onClick={() => setPaths([])}>
                  Clear selection
                </Button>
              </section>
            )}
            <section className="panel">
              <h2>Conversion route</h2>
              <div className="row">
                <label htmlFor="target">
                  Output format
                  <select id="target" value={target} onChange={(e) => setTarget(e.target.value)}>
                    {[...new Set([...formats, ...(inspection?.targets ?? []), target])].map((f) => (
                      <option key={f}>{f}</option>
                    ))}
                  </select>
                </label>
                {selectProfile(profile, setProfile, "profile")}
              </div>
              <div className="row">
                <div className="output-location">
                  Output folder<span className="path">{output ?? "Beside each input file"}</span>
                </div>
                <Button variant="outline" onClick={() => void run(() => chooseFolder(setOutput))}>
                  Choose output folder
                </Button>
                <Button variant="ghost" onClick={() => setOutput(null)}>
                  Use input folder
                </Button>
              </div>
              <label className="check">
                <input
                  type="checkbox"
                  checked={overwrite}
                  onChange={(e) => setOverwrite(e.target.checked)}
                />
                Replace existing output files
              </label>
              {inspection && (
                <div className="route-note">
                  <strong>{inspection.summary}</strong>
                  <p>{inspection.explanation}</p>
                  {inspection.warnings.map((w) => (
                    <p key={w}>{w}</p>
                  ))}
                  <small>
                    Available targets: {inspection.targets.join(", ") || "none"}. Summary shown for
                    the first selected file.
                  </small>
                </div>
              )}
              <Button
                size="lg"
                aria-label="Start conversion"
                aria-keyshortcuts={ariaShortcut("Enter")}
                disabled={!canConvert}
                onClick={() => void start()}
              >
                {working
                  ? "Adding to queue…"
                  : `Convert${paths.length > 1 ? ` ${paths.length} files` : ""}`}
                {!working && <Kbd>{shortcutLabel("Enter")}</Kbd>}
              </Button>
            </section>
          </>
        )}
        {tab === "Queue" && (
          <section className="panel">
            <div className="row queue-head">
              <h2>Conversion queue</h2>
              <div className="actions">
                {summary.failed > 0 && (
                  <Button
                    variant="outline"
                    onClick={() =>
                      void run(async () => {
                        for (const j of jobs)
                          if (j.status === "failed") await commands.retry({ id: j.id });
                      })
                    }
                  >
                    Retry failed ({summary.failed})
                  </Button>
                )}
                <Button
                  variant="outline"
                  aria-keyshortcuts={ariaShortcut(".")}
                  disabled={!summary.active}
                  onClick={cancelPending}
                >
                  Cancel pending jobs <Kbd>{shortcutLabel(".")}</Kbd>
                </Button>
              </div>
            </div>
            <p role="status" className="queue-summary">
              {queueText(summary)}
            </p>
            {summary.active > 0 && (
              <progress
                aria-label="Batch progress"
                max={summary.batchTotal}
                value={summary.batchDone}
              />
            )}
            <p>
              Concurrency: {state?.settings.concurrency ?? 2}. Engine sessions stay warm between
              jobs.
            </p>
            {state && !jobs.length && (
              <div className="empty-state">
                <h3>No conversions yet</h3>
                <p>Files you convert appear here with their progress and results.</p>
                <Button
                  aria-keyshortcuts={ariaShortcut("o")}
                  onClick={() => {
                    setTab("Convert");
                    chooseFiles();
                  }}
                >
                  Choose files <Kbd>{shortcutLabel("o")}</Kbd>
                </Button>
              </div>
            )}
            <ul className="jobs">
              {orderJobs(jobs).map((job) => (
                <li key={job.id} data-status={job.status}>
                  <div>
                    <strong title={job.input}>{name(job.input)}</strong>
                    <p>
                      <StatusBadge status={job.status} /> {job.target.toUpperCase()} · {job.profile}
                    </p>
                    <p role="status">{job.stage}</p>
                    {job.error && <p className="error">{job.error}</p>}
                    {job.warnings.map((w) => (
                      <p key={w}>{w}</p>
                    ))}
                    <small title={job.output}>{job.output}</small>
                  </div>
                  <div className="actions">
                    {isActive(job.status) && (
                      <Button
                        variant="outline"
                        onClick={() => void run(() => commands.cancel({ id: job.id }))}
                      >
                        Cancel
                      </Button>
                    )}
                    {job.error && (
                      <Button variant="ghost" onClick={() => copyError(job.error ?? "")}>
                        Copy error
                      </Button>
                    )}
                    {(job.status === "failed" || job.status === "cancelled") && (
                      <Button
                        variant="outline"
                        onClick={() => void run(() => commands.retry({ id: job.id }))}
                      >
                        Retry
                      </Button>
                    )}
                    {job.status === "succeeded" && (
                      <Button
                        variant="outline"
                        onClick={() => void run(() => revealItemInDir(job.output))}
                      >
                        Show output
                      </Button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          </section>
        )}
        {tab === "Watch folders" && (
          <>
            <section className="panel">
              <h2>A saved route for incoming documents</h2>
              <p>
                New and changed files are converted after two stable scans. Watches continue while
                this app is open; changes made while paused are ignored.
              </p>
              <div className="row">
                <label htmlFor="watch-input">
                  Input folder
                  <input
                    id="watch-input"
                    value={watchInput}
                    onChange={(e) => setWatchInput(e.target.value)}
                  />
                </label>
                <Button
                  variant="outline"
                  onClick={() => void run(() => chooseFolder(setWatchInput))}
                >
                  Choose watched folder
                </Button>
              </div>
              <div className="row">
                <label htmlFor="watch-output">
                  Output folder
                  <input
                    id="watch-output"
                    value={watchOutput}
                    onChange={(e) => setWatchOutput(e.target.value)}
                  />
                </label>
                <Button
                  variant="outline"
                  onClick={() => void run(() => chooseFolder(setWatchOutput))}
                >
                  Choose watch output
                </Button>
              </div>
              <div className="row">
                <label htmlFor="watch-from">
                  Input format
                  <select
                    id="watch-from"
                    value={watchFrom}
                    onChange={(e) => setWatchFrom(e.target.value)}
                  >
                    {Object.keys(inputExtensions).map((f) => (
                      <option key={f}>{f}</option>
                    ))}
                  </select>
                </label>
                <label htmlFor="watch-target">
                  Output format
                  <select
                    id="watch-target"
                    value={watchTarget}
                    onChange={(e) => setWatchTarget(e.target.value)}
                  >
                    {formats.map((f) => (
                      <option key={f}>{f}</option>
                    ))}
                  </select>
                </label>
                {selectProfile(watchProfile, setWatchProfile, "watch-profile")}
              </div>
              <label htmlFor="watch-interval">
                Polling interval (seconds)
                <input
                  id="watch-interval"
                  type="number"
                  min="1"
                  max="3600"
                  value={watchInterval}
                  onChange={(e) => setWatchInterval(Number(e.target.value))}
                />
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={watchRecursive}
                  onChange={(e) => setWatchRecursive(e.target.checked)}
                />
                Include subfolders
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={watchOverwrite}
                  onChange={(e) => setWatchOverwrite(e.target.checked)}
                />
                Replace previous watch outputs
              </label>
              <Button
                disabled={!watchInput || !watchOutput || watchFrom === watchTarget}
                onClick={() =>
                  void run(() =>
                    commands.watch({
                      interval_seconds: watchInterval,
                      recursive: watchRecursive,
                      input: watchInput,
                      output: watchOutput,
                      from: watchFrom,
                      target: watchTarget,
                      profile: watchProfile,
                      overwrite: watchOverwrite,
                    }),
                  )
                }
              >
                Start watching
              </Button>
            </section>
            <section className="panel">
              <h2>Watched folders</h2>
              {state && !state.watches.length && (
                <div className="empty-state">
                  <h3>No folders are watched yet</h3>
                  <p>
                    Choose a watched folder and a separate output folder above. New documents there
                    are converted while AriadShift is open.
                  </p>
                </div>
              )}
              {state?.watches.map((watch) => (
                <article key={watch.id} className="watch">
                  <strong>{watch.route.input}</strong>
                  <p>
                    {watch.route.from} → {watch.route.target} · {watch.route.profile} ·{" "}
                    {watch.paused ? "Paused" : "Watching"}
                  </p>
                  <p>Output: {watch.route.output}</p>
                  {watch.error && <p className="error">{watch.error}</p>}
                  <div className="actions">
                    <Button
                      variant="outline"
                      onClick={() =>
                        void run(() =>
                          commands.watch_control({
                            id: watch.id,
                            paused: !watch.paused,
                            remove: false,
                          }),
                        )
                      }
                    >
                      {watch.paused ? "Resume" : "Pause"}
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() =>
                        void run(() =>
                          commands.watch_control({ id: watch.id, paused: true, remove: true }),
                        )
                      }
                    >
                      Remove watch
                    </Button>
                  </div>
                </article>
              ))}
            </section>
          </>
        )}
        {tab === "Engine packs" && (
          <>
            <section className="panel">
              <h2>Add an engine when you need it</h2>
              <p>
                The base app excludes Docling. Downloads require a trusted signature and verified
                hashes.
              </p>
              <div className="row">
                <label htmlFor="pack-name">
                  Pack name
                  <input
                    id="pack-name"
                    value={packName}
                    onChange={(e) => setPackName(e.target.value)}
                  />
                </label>
                <label htmlFor="pack-from">
                  Local source or manifest URL (optional)
                  <input
                    id="pack-from"
                    value={packFrom}
                    onChange={(e) => setPackFrom(e.target.value)}
                    placeholder="Same source as ashift engines install --from"
                  />
                </label>
                <Button variant="outline" onClick={() => void run(() => chooseFolder(setPackFrom))}>
                  Choose local source
                </Button>
              </div>
              <Button
                disabled={state?.pack_busy || !packName}
                onClick={() => void packAction("install", { name: packName })}
              >
                Install pack
              </Button>
              <p role="status">{state?.pack_progress}</p>
              {state?.pack_busy && (
                <Button variant="outline" onClick={() => void run(() => commands.pack_cancel({}))}>
                  Cancel pack operation
                </Button>
              )}
            </section>
            <section className="panel">
              <h2>Available and installed</h2>
              {!packs && <p role="status">Loading engine packs…</p>}
              {packs && !packs.length && (
                <div className="empty-state">
                  <h3>No engine packs listed</h3>
                  <p>
                    The pack index could not list any pack. Install from a local source above, or
                    try again when you are online.
                  </p>
                  <Button variant="outline" onClick={() => void run(loadPacks)}>
                    Reload packs
                  </Button>
                </div>
              )}
              {packs?.map((pack) => (
                <article className="watch" key={`${pack.name}-${pack.version}`}>
                  <strong>
                    {pack.name} {pack.version}
                  </strong>
                  <p>
                    {pack.description} ·{" "}
                    {pack.installed
                      ? pack.active
                        ? "Installed · active"
                        : "Installed"
                      : "Available"}
                  </p>
                  <div className="actions">
                    {pack.installed ? (
                      <>
                        <Button
                          disabled={state?.pack_busy}
                          variant="outline"
                          onClick={() => void packAction("verify", pack)}
                        >
                          Verify
                        </Button>
                        <Button
                          disabled={state?.pack_busy}
                          variant="outline"
                          onClick={() => void packAction("remove", pack)}
                        >
                          Remove pack
                        </Button>
                      </>
                    ) : (
                      <Button
                        disabled={state?.pack_busy}
                        variant="outline"
                        onClick={() => void packAction("install", pack)}
                      >
                        Install {pack.name}
                      </Button>
                    )}
                  </div>
                </article>
              ))}
            </section>
          </>
        )}
        {tab === "Diagnostics" && (
          <section className="panel">
            <h2>Environment health</h2>
            <Button
              disabled={checking}
              onClick={() =>
                void run(async () => {
                  setChecking(true);
                  try {
                    setDiagnostics(await commands.doctor({}));
                  } finally {
                    setChecking(false);
                  }
                })
              }
            >
              {checking ? "Checking…" : diagnostics ? "Run diagnostics again" : "Run diagnostics"}
            </Button>
            {!diagnostics && (
              <p>
                Checks Pandoc, LibreOffice, engine packs, the workspace and the sandbox. Nothing
                leaves this computer.
              </p>
            )}
            {diagnostics && <p role="status">{diagnosticsText(diagnostics)}</p>}
            <ul>
              {diagnostics?.map((d) => (
                <li className="diagnostic" key={d.id}>
                  <Badge variant={d.ok ? "default" : d.required ? "destructive" : "outline"}>
                    {d.ok ? "OK" : d.required ? "Required" : "Optional"}
                  </Badge>
                  <div>
                    <strong>{d.message}</strong>
                    {d.hint && <p>{d.hint}</p>}
                  </div>
                </li>
              ))}
            </ul>
          </section>
        )}
        {tab === "Settings" && settings && (
          <>
            <section className="panel">
              <h2>Defaults</h2>
              {selectProfile(
                settings.profile,
                (profile) => setSettings({ ...settings, profile }),
                "default-profile",
              )}
              <label htmlFor="concurrency">
                Concurrent conversions
                <select
                  id="concurrency"
                  value={settings.concurrency}
                  onChange={(e) =>
                    setSettings({ ...settings, concurrency: Number(e.target.value) })
                  }
                >
                  {[1, 2, 3, 4].map((n) => (
                    <option key={n}>{n}</option>
                  ))}
                </select>
              </label>
              <label htmlFor="output-policy">
                Output location
                <select
                  id="output-policy"
                  value={settings.output_policy}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      output_policy: e.target.value as Settings["output_policy"],
                    })
                  }
                >
                  <option value="adjacent">Beside input files</option>
                  <option value="folder">Default folder</option>
                </select>
              </label>
              {settings.output_policy === "folder" && (
                <div className="row">
                  <label htmlFor="default-output">
                    Default output folder
                    <input
                      id="default-output"
                      value={settings.output_folder ?? ""}
                      onChange={(e) => setSettings({ ...settings, output_folder: e.target.value })}
                    />
                  </label>
                  <Button
                    variant="outline"
                    onClick={() =>
                      void run(() =>
                        chooseFolder((output_folder) =>
                          setSettings({ ...settings, output_folder }),
                        ),
                      )
                    }
                  >
                    Choose default folder
                  </Button>
                </div>
              )}
              <label className="check">
                <input
                  type="checkbox"
                  checked={settings.overwrite}
                  onChange={(e) => setSettings({ ...settings, overwrite: e.target.checked })}
                />
                Replace existing output files by default
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={settings.telemetry}
                  onChange={(e) => setSettings({ ...settings, telemetry: e.target.checked })}
                />
                Opt in to telemetry
              </label>
              <p>No telemetry collection is implemented. This preference is stored locally only.</p>
              <div className="row save-row">
                <Button
                  disabled={!settingsChanged}
                  onClick={() =>
                    void run(async () => {
                      const saved = await commands.settings(settings);
                      setSettings(saved);
                      setSavedSettings(saved);
                      setProfile(saved.profile);
                      setOverwrite(saved.overwrite);
                      setOutput(saved.output_policy === "folder" ? saved.output_folder : null);
                      setNotice(info("Settings saved"));
                    })
                  }
                >
                  Save settings
                </Button>
                {settingsChanged && (
                  <>
                    <Button variant="ghost" onClick={() => setSettings(savedSettings)}>
                      Discard changes
                    </Button>
                    <span className="unsaved">Unsaved changes</span>
                  </>
                )}
              </div>
            </section>
            <section className="panel">
              <h2>App updates</h2>
              <p>
                Updates are checked at startup and verified with the bundled public key.
                Installation starts only when you choose it.
              </p>
              <Button
                variant="outline"
                disabled={working}
                onClick={() =>
                  void run(async () => {
                    const found = await check();
                    setUpdate(found);
                    setUpdateProgress(
                      found ? `Version ${found.version} is available` : "You are up to date",
                    );
                  })
                }
              >
                Check for updates
              </Button>
              <p role="status">{updateProgress}</p>
              {update && (
                <Button
                  disabled={working}
                  onClick={() =>
                    void run(async () => {
                      setWorking(true);
                      try {
                        await update.downloadAndInstall((event) =>
                          setUpdateProgress(
                            event.event === "Progress" ? "Downloading update…" : event.event,
                          ),
                        );
                        setUpdateProgress("Update installed. Close and reopen AriadShift.");
                        setUpdate(null);
                      } finally {
                        setWorking(false);
                      }
                    })
                  }
                >
                  Install update {update.version}
                </Button>
              )}
            </section>
            <section className="panel">
              <h2>Keyboard shortcuts</h2>
              <dl className="shortcuts">
                {shortcutList(tabs).map((item) => (
                  <div key={item.action} className="shortcut">
                    <dt>
                      <kbd className="kbd">{item.keys}</kbd>
                    </dt>
                    <dd>{item.action}</dd>
                  </div>
                ))}
              </dl>
            </section>
          </>
        )}
        <footer>AriadShift · Private by default · Open source</footer>
      </main>
      <Toasts toasts={toasts} onDismiss={dismissToast} />
      {dragging && <DropOverlay dragging={dragging} />}
    </div>
  );
}
