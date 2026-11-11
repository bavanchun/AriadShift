"use client";
// The workspace controller: the file set, per-file insights, the active tool, tool runs and
// the result that follows, with promotion of an output to the next input and undo.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openOutput } from "@/converter/client";
import { STORE_INPUT_LIMIT_BYTES } from "@/converter/file-store";
import type { InputIngestResult, ToolExecutionArtifact } from "@/converter/protocol";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import type { PageToolCall } from "./page-edits";
import { useToolLabel } from "./tool-label";
import type {
  InsightsState,
  RecipeStep,
  ThreadStep,
  WorkspaceClient,
  WorkspaceFile,
  WorkspaceRun,
} from "./types";

export interface UseWorkspaceOptions {
  client: WorkspaceClient;
  /** Listens for worker progress messages; returns an unsubscribe function. */
  onProgress?: (listener: (message: string) => void) => () => void;
  /** Largest total input a tool may read. */
  budgetBytes?: number;
}

let keys = 0;
const nextKey = () => `file-${++keys}`;

const isPdf = (file: { name: string }, result?: InputIngestResult) =>
  result?.inspection?.format === ("pdf" as string) || /\.pdf$/i.test(file.name);

const message = (error: unknown) =>
  typeof error === "object" && error !== null && "message" in error
    ? String((error as { message: unknown }).message)
    : String(error);

const cancelled = (error: unknown) =>
  typeof error === "object" && error !== null && (error as { code?: unknown }).code === "cancelled";

type Busy = "ingest" | "run" | null;

/** How a chain of tool steps ended; a failed one carries the message that was also shown. */
export type ChainOutcome = { ok: true } | { ok: false; message: string };

export function useWorkspace({
  client,
  onProgress,
  budgetBytes = STORE_INPUT_LIMIT_BYTES,
}: UseWorkspaceOptions) {
  const toolLabel = useToolLabel();
  const [files, setFiles] = useState<WorkspaceFile[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [insights, setInsights] = useState<Record<string, InsightsState>>({});
  const [tools, setTools] = useState<ToolDescriptor[]>([]);
  const [toolId, setToolId] = useState<string | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [run, setRun] = useState<WorkspaceRun | null>(null);
  const [chain, setChain] = useState<RecipeStep[]>([]);
  const [thread, setThread] = useState<ThreadStep[]>([]);
  // Earlier file sets, newest last, so a promoted output can be undone.
  const history = useRef<WorkspaceFile[][]>([]);
  const [canUndo, setCanUndo] = useState(false);
  // Every file-set change bumps this; a slower, older analysis must not overwrite newer state.
  const generation = useRef(0);
  const live = useRef({ files, activeId });
  live.current = { files, activeId };

  useEffect(() => {
    let current = true;
    client.listTools().then(
      (list) => current && setTools(list),
      () => undefined,
    );
    return () => {
      current = false;
    };
  }, [client]);

  useEffect(() => {
    if (!(onProgress && busy === "run")) return;
    return onProgress(setProgress);
  }, [onProgress, busy]);

  const analyse = useCallback(
    async (set: WorkspaceFile[], results: InputIngestResult[], token: number) => {
      for (const [index, file] of set.entries()) {
        if (!isPdf(file, results[index])) continue;
        setInsights((previous) => ({ ...previous, [file.id]: { status: "loading" } }));
        try {
          const read = await client.inspectInsights(file.id);
          if (token !== generation.current) return;
          setInsights((previous) => ({
            ...previous,
            [file.id]: { status: "ready", insights: read },
          }));
          setFiles((previous) =>
            previous.map((entry) =>
              entry.id === file.id ? { ...entry, pageCount: read.pageCount } : entry,
            ),
          );
        } catch (reason) {
          if (token !== generation.current) return;
          setInsights((previous) => ({
            ...previous,
            [file.id]: { status: "failed", message: message(reason) },
          }));
        }
      }
    },
    [client],
  );

  /** Replaces the converter's input set with these files, keeping each file's identity. */
  const ingest = useCallback(
    async (next: Array<{ file: File; key?: string }>, preferredActive?: number) => {
      const token = ++generation.current;
      setBusy("ingest");
      setError(null);
      try {
        const results = await client.ingestMany(next.map((entry) => entry.file));
        if (token !== generation.current) return;
        const set: WorkspaceFile[] = next.map((entry, index) => {
          const result = results[index];
          return {
            id: result?.id ?? "",
            key: entry.key ?? nextKey(),
            name: entry.file.name,
            size: entry.file.size,
            pageCount: null,
            file: entry.file,
            ...(result?.inspectionError ? { problem: result.inspectionError.message } : {}),
          };
        });
        setFiles(set);
        setInsights({});
        setActiveId(set[Math.min(preferredActive ?? 0, set.length - 1)]?.id ?? null);
        setBusy(null);
        void analyse(set, results, token);
        return set;
      } catch (reason) {
        if (token === generation.current) {
          setError(message(reason));
          setBusy(null);
        }
      }
    },
    [client, analyse],
  );

  const addFiles = useCallback(
    async (added: File[]) => {
      history.current = [];
      setCanUndo(false);
      setRun(null);
      setChain([]);
      setThread([]);
      const kept = live.current.files.map((entry) => ({ file: entry.file, key: entry.key }));
      await ingest([...kept, ...added.map((file) => ({ file }))], kept.length);
    },
    [ingest],
  );

  const removeFile = useCallback(
    async (id: string) => {
      const remaining = live.current.files.filter((entry) => entry.id !== id);
      setRun(null);
      if (remaining.length === 0) {
        generation.current += 1;
        await client.cancel();
        setFiles([]);
        setActiveId(null);
        setInsights({});
        return;
      }
      await ingest(remaining.map((entry) => ({ file: entry.file, key: entry.key })));
    },
    [client, ingest],
  );

  const moveFile = useCallback((id: string, offset: -1 | 1) => {
    setFiles((previous) => {
      const index = previous.findIndex((entry) => entry.id === id);
      const target = index + offset;
      if (index < 0 || target < 0 || target >= previous.length) return previous;
      const next = [...previous];
      const [entry] = next.splice(index, 1);
      if (entry) next.splice(target, 0, entry);
      return next;
    });
  }, []);

  const active = useMemo(
    () => files.find((entry) => entry.id === activeId) ?? null,
    [files, activeId],
  );
  const tool = useMemo(() => tools.find((entry) => entry.id === toolId) ?? null, [tools, toolId]);

  const inputIdsFor = useCallback(
    (descriptor: ToolDescriptor) =>
      descriptor.inputs.arity === "many"
        ? live.current.files.map((entry) => entry.id)
        : live.current.activeId
          ? [live.current.activeId]
          : [],
    [],
  );

  /** Re-ingests files after the worker was replaced (cancel) or an intermediate step. */
  const restore = useCallback(
    async (set: WorkspaceFile[], activeKey: string | undefined) => {
      const index = Math.max(
        0,
        set.findIndex((entry) => entry.key === activeKey),
      );
      await ingest(
        set.map((entry) => ({ file: entry.file, key: entry.key })),
        index,
      );
    },
    [ingest],
  );

  const executeChain = useCallback(
    async (
      steps: Array<{ tool: ToolDescriptor; params: Record<string, unknown> }>,
      options: { inputIds?: string[] } = {},
    ): Promise<ChainOutcome> => {
      const original = live.current.files;
      const originalActive = original.find((entry) => entry.id === live.current.activeId);
      const started = steps.map((step, index) => ({
        id: `${step.tool.id}:${chain.length + index}`,
        label: toolLabel(step.tool),
        status: "pending" as ThreadStep["status"],
      }));
      const completed = chain.map((step, index) => ({
        id: `done:${index}`,
        label: toolLabel({ id: step.toolId }),
        status: "completed" as ThreadStep["status"],
      }));
      const setStatus = (index: number, status: ThreadStep["status"]) => {
        started[index] = { ...(started[index] as ThreadStep), status };
        setThread([...completed, ...started]);
      };
      const first = steps[0];
      if (!first) return { ok: false, message: "There is nothing to run." };
      setError(null);
      setRun(null);
      setBusy("run");
      setProgress(null);
      setThread([...completed, ...started]);
      const inputs = options.inputIds?.length ? options.inputIds : inputIdsFor(first.tool);
      const sources = inputs.map((id) => original.find((entry) => entry.id === id));
      const input = {
        size: sources.reduce((sum, entry) => sum + (entry?.size ?? 0), 0),
        pageCount: sources.length === 1 ? (sources[0]?.pageCount ?? null) : null,
        names: sources.map((entry) => entry?.name ?? ""),
      };
      let slot = inputs;
      let last: WorkspaceRun | null = null;
      try {
        for (const [index, step] of steps.entries()) {
          setStatus(index, "running");
          const result = await client.runTool(step.tool.id, step.params, {
            inputIds: slot,
            budgetBytes,
          });
          setStatus(index, "completed");
          last = {
            tool: step.tool,
            params: step.params,
            result,
            input,
            sourceKey: originalActive?.key,
            redacts: steps.some((entry) => entry.tool.id === "pdf.redact"),
          };
          if (index < steps.length - 1) {
            // The next step reads this step's output, which must become an input slot.
            const primary = result.artifacts[0];
            const blob = await openOutput(result.primaryLocation);
            const file = new File([blob], primary?.name ?? "output.pdf", {
              type: result.mediaType,
            });
            const [stored] = await client.ingestMany([file]);
            slot = stored ? [stored.id] : [];
          }
        }
        if (steps.length > 1) await restore(original, originalActive?.key);
        setRun(last);
        setBusy(null);
        setProgress(null);
        return { ok: true };
      } catch (reason) {
        const index = started.findIndex((entry) => entry.status === "running");
        if (cancelled(reason)) {
          setThread(completed);
          // Cancelling replaced the worker, so its stored inputs are gone.
          await restore(original, originalActive?.key);
          // Re-ingesting may be superseded by a newer file change, which would leave "run" set.
          setBusy(null);
        } else {
          if (index >= 0) setStatus(index, "failed");
          setError(message(reason));
          if (steps.length > 1) await restore(original, originalActive?.key);
          setBusy(null);
        }
        setProgress(null);
        return { ok: false, message: message(reason) };
      }
    },
    [chain, client, budgetBytes, inputIdsFor, restore, toolLabel],
  );

  const runTool = useCallback(
    (
      descriptor: ToolDescriptor,
      params: Record<string, unknown>,
      options?: { inputIds?: string[] },
    ) => executeChain([{ tool: descriptor, params }], options),
    [executeChain],
  );

  /** Runs planned tool calls on the active PDF in order, with one result at the end. */
  const applyCalls = useCallback(
    async (
      calls: Array<{ toolId: string; params: Record<string, unknown> }>,
    ): Promise<ChainOutcome> => {
      const steps = calls.flatMap((call) => {
        const descriptor = tools.find((entry) => entry.id === call.toolId);
        return descriptor ? [{ tool: descriptor, params: call.params }] : [];
      });
      if (steps.length !== calls.length)
        throw new Error("A page tool is missing from the catalogue.");
      return executeChain(steps);
    },
    [tools, executeChain],
  );

  /** Runs the page grid's planned calls on the active PDF, one result at the end. */
  const applyPageEdits = useCallback(
    async (calls: PageToolCall[]) => {
      await applyCalls(calls);
    },
    [applyCalls],
  );

  const cancel = useCallback(() => client.cancel(), [client]);

  /** Makes an output the workspace's input, remembering the files it replaces. */
  const promote = useCallback(
    async (artifact: ToolExecutionArtifact) => {
      const blob = await openOutput(artifact.location);
      const file = new File([blob], artifact.name, { type: artifact.mediaType });
      const previous = live.current.files;
      const finished = run;
      const stored = await ingest([{ file }]);
      if (!stored) return;
      history.current.push(previous);
      setCanUndo(true);
      if (finished)
        setChain((steps) => [...steps, { toolId: finished.tool.id, params: finished.params }]);
      setRun(null);
      setThread((steps) => steps.map((step) => ({ ...step, status: "completed" as const })));
    },
    [ingest, run],
  );

  /** Discards a shown result; with none shown, restores the files a promotion replaced. */
  const undo = useCallback(async () => {
    if (run) {
      setRun(null);
      setThread((steps) => steps.slice(0, Math.max(0, steps.length - 1)));
      return;
    }
    const previous = history.current.pop();
    setCanUndo(history.current.length > 0);
    if (!previous) return;
    setChain((steps) => steps.slice(0, -1));
    setThread((steps) => steps.slice(0, -1));
    await ingest(previous.map((entry) => ({ file: entry.file, key: entry.key })));
  }, [run, ingest]);

  const recipe: RecipeStep[] = useMemo(
    () => [...chain, ...(run ? [{ toolId: run.tool.id, params: run.params }] : [])],
    [chain, run],
  );

  return {
    files,
    active,
    activeId,
    insights,
    tools,
    tool,
    busy,
    progress,
    error,
    run,
    thread,
    recipe,
    canUndo: canUndo || run !== null,
    addFiles,
    removeFile,
    moveFile,
    selectFile: setActiveId,
    openTool: setToolId,
    runTool,
    applyCalls,
    applyPageEdits,
    cancel,
    promote,
    undo,
    dismissError: () => setError(null),
  };
}
