"use client";

import { Button } from "@ariadshift/ui/components/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@ariadshift/ui/components/tabs";
import { ToastProvider, ToastViewport } from "@ariadshift/ui/components/toast";
import { Undo2Icon } from "lucide-react";
import { type ReactNode, useMemo } from "react";
import type { StorageKind, ToolExecutionArtifact } from "@/converter/protocol";
import { resolveWasmUrl } from "@/viewer/engine";
import { loadEmbedPdfBackend, PdfViewer, useViewerBackend } from "@/viewer/pdf-viewer";
import type { PdfViewerSource, ViewerBackendLoader } from "@/viewer/types";
import { useWorkspaceCopy, type WorkspaceCopyOverrides, WorkspaceCopyProvider } from "./copy";
import { FileTray } from "./file-tray";
import { GoldenThreadBar } from "./golden-thread";
import { FileInsightsPanel } from "./insights-panel";
import type { PageToolCall } from "./page-edits";
import { PageGrid } from "./page-grid";
import { ToolResultCard } from "./result-card";
import { ToolInspector } from "./tool-inspector";
import { toolLabel } from "./tool-label";
import type { RecipeStep, WorkspaceClient } from "./types";
import { useWorkspace } from "./use-workspace";

export interface DocumentWorkspaceProps {
  client: WorkspaceClient;
  /** Listens for worker progress messages; returns an unsubscribe function. */
  onProgress?: (listener: (message: string) => void) => () => void;
  locale?: string;
  /** The configured cloud API; "Run in Cloud" needs this and `onRunCloud`. */
  cloudApiUrl?: string;
  onRunCloud?: (toolId: string, params: Record<string, unknown>) => void | Promise<void>;
  storage?: StorageKind;
  copy?: WorkspaceCopyOverrides;
  loadBackend?: ViewerBackendLoader;
  /** Largest total input a tool may read. */
  budgetBytes?: number;
  /** Offered on a result only when set. */
  onOpenInEditor?: (artifact: ToolExecutionArtifact) => void;
  onSaveRecipe?: (steps: RecipeStep[]) => void;
}

/** Files, the active PDF's preview and page grid, and a tool with its result, in one place. */
export function DocumentWorkspace(props: DocumentWorkspaceProps) {
  return (
    <WorkspaceCopyProvider copy={props.copy}>
      <ToastProvider>
        <WorkspaceBody {...props} />
        <ToastViewport />
      </ToastProvider>
    </WorkspaceCopyProvider>
  );
}

function WorkspaceBody({
  client,
  onProgress,
  locale,
  cloudApiUrl,
  onRunCloud,
  storage,
  loadBackend = loadEmbedPdfBackend,
  budgetBytes,
  onOpenInEditor,
  onSaveRecipe,
}: DocumentWorkspaceProps) {
  const copy = useWorkspaceCopy();
  const workspace = useWorkspace({ client, onProgress, budgetBytes });
  const { active, tool, run } = workspace;
  const insightsState = active ? workspace.insights[active.id] : undefined;
  const source: PdfViewerSource | null = useMemo(
    () =>
      active && insightsState && insightsState.status !== "failed"
        ? { id: active.key, name: active.name, data: active.file }
        : null,
    [active, insightsState],
  );
  const backend = useViewerBackend(loadBackend);
  const insights = insightsState?.status === "ready" ? insightsState.insights : null;

  return (
    <div
      data-slot="document-workspace"
      className="grid min-w-0 gap-4 lg:grid-cols-[18rem_minmax(0,1fr)_24rem]"
    >
      <div className="flex flex-col gap-4">
        <FileTray
          files={workspace.files}
          activeId={workspace.activeId}
          storage={storage ?? undefined}
          busy={workspace.busy === "ingest"}
          locale={locale}
          onAddFiles={workspace.addFiles}
          onRemove={workspace.removeFile}
          onMove={workspace.moveFile}
          onSelect={workspace.selectFile}
        />
        <div className="flex flex-col gap-1">
          <label htmlFor="workspace-tool" className="text-sm font-medium">
            {copy.workspace.toolPicker}
          </label>
          <ToolPicker
            id="workspace-tool"
            tools={workspace.tools}
            value={tool?.id ?? ""}
            onChange={workspace.openTool}
          />
        </div>
      </div>

      <div className="flex min-w-0 flex-col gap-4">
        {active && insightsState && (
          <FileInsightsPanel
            state={insightsState}
            tools={workspace.tools}
            onOpenTool={workspace.openTool}
          />
        )}
        {source ? (
          <Tabs defaultValue="preview">
            <TabsList>
              <TabsTrigger value="preview">{copy.workspace.viewer}</TabsTrigger>
              <TabsTrigger value="pages">{copy.workspace.pagesTab}</TabsTrigger>
            </TabsList>
            <TabsContent value="preview">
              <PdfViewer source={source} loadBackend={loadBackend} className="h-[70vh]" />
            </TabsContent>
            <TabsContent value="pages">
              {insights ? (
                <PagesPanel
                  key={source.id}
                  source={source}
                  backend={backend}
                  pageCount={insights.pageCount}
                  blankPages={insights.blankPages}
                  scannedPages={insights.scannedPages}
                  busy={workspace.busy !== null}
                  onApply={workspace.applyPageEdits}
                />
              ) : null}
            </TabsContent>
          </Tabs>
        ) : (
          <p className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
            {copy.workspace.noPdf}
          </p>
        )}
      </div>

      <div className="flex min-w-0 flex-col gap-4">
        {workspace.thread.length > 0 && <GoldenThreadBar steps={workspace.thread} />}
        <ToolInspector
          tool={tool}
          files={workspace.files}
          pageCount={active?.pageCount ?? undefined}
          cloudApiUrl={cloudApiUrl}
          locale={locale}
          busy={workspace.busy === "run"}
          progress={workspace.progress}
          error={workspace.error}
          onRun={(params) => (tool ? workspace.runTool(tool, params) : undefined)}
          onRunCloud={onRunCloud && tool ? (params) => onRunCloud(tool.id, params) : undefined}
          onCancel={workspace.cancel}
        />
        {run && (
          <ToolResultCard
            run={run}
            locale={locale}
            onContinue={workspace.promote}
            onUndo={workspace.undo}
            onOpenInEditor={onOpenInEditor}
            onSaveRecipe={onSaveRecipe ? () => onSaveRecipe(workspace.recipe) : undefined}
          />
        )}
        {!run && workspace.canUndo && (
          <Button variant="ghost" size="sm" className="self-start" onClick={workspace.undo}>
            <Undo2Icon aria-hidden="true" />
            {copy.result.undo}
          </Button>
        )}
      </div>
    </div>
  );
}

function PagesPanel({
  source,
  backend,
  pageCount,
  blankPages,
  scannedPages,
  busy,
  onApply,
}: {
  source: PdfViewerSource;
  backend: ReturnType<typeof useViewerBackend>;
  pageCount: number;
  blankPages: number[];
  scannedPages: number[];
  busy: boolean;
  onApply: (calls: PageToolCall[]) => void | Promise<void>;
}) {
  const grid = (renderPreview?: (page: number) => ReactNode) => (
    <PageGrid
      pageCount={pageCount}
      blankPages={blankPages}
      scannedPages={scannedPages}
      renderPreview={renderPreview}
      busy={busy}
      onApply={onApply}
    />
  );
  const wasmUrl = useMemo(() => {
    try {
      return resolveWasmUrl();
    } catch {
      return null;
    }
  }, []);
  if (backend.status !== "ready" || !wasmUrl) return grid();
  const { PagePreviewProvider } = backend.backend;
  return (
    <PagePreviewProvider source={source} wasmUrl={wasmUrl}>
      {(renderPreview) => grid(renderPreview)}
    </PagePreviewProvider>
  );
}

function ToolPicker({
  id,
  tools,
  value,
  onChange,
}: {
  id: string;
  tools: Array<{ id: string; group: string }>;
  value: string;
  onChange: (id: string) => void;
}) {
  const groups = useMemo(() => {
    const byGroup = new Map<string, Array<{ id: string; group: string }>>();
    for (const entry of tools)
      byGroup.set(entry.group, [...(byGroup.get(entry.group) ?? []), entry]);
    return [...byGroup.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [tools]);
  return (
    <select
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      className="h-9 w-full rounded-lg border border-input bg-background px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
    >
      <option value="" />
      {groups.map(([group, entries]) => (
        <optgroup key={group} label={group}>
          {entries.map((entry) => (
            <option key={entry.id} value={entry.id}>
              {toolLabel(entry)}
            </option>
          ))}
        </optgroup>
      ))}
    </select>
  );
}
