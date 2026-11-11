"use client";

import { Button } from "@ariadshift/ui/components/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@ariadshift/ui/components/tabs";
import { ToastProvider, ToastViewport } from "@ariadshift/ui/components/toast";
import { DownloadIcon, Undo2Icon } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { AiPanel } from "@/components/ai";
import type { ParamsValidation } from "@/components/tools/params-form";
import { downloadOutput } from "@/converter/client";
import type { StorageKind, ToolExecutionArtifact } from "@/converter/protocol";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import type { EditorToolCall } from "@/editor/annotate";
import type { ExportRequest } from "@/editor/document";
import type { EditApplyParams } from "@/editor/page";
import { resolveWasmUrl } from "@/viewer/engine";
import { loadEmbedPdfBackend, PdfViewer, useViewerBackend } from "@/viewer/pdf-viewer";
import type { PdfViewerSource, ViewerBackendLoader, ViewerLabels } from "@/viewer/types";
import { BatchSection } from "./batch-section";
import { useWorkspaceCopy, type WorkspaceCopyOverrides, WorkspaceCopyProvider } from "./copy";
import { type EditorExport, type EditorLocationState, exportEditedIr } from "./editor-data";
import { AnnotatePane, DocumentPane, PagePane, WorkspaceModeSwitch } from "./editor-panes";
import { FileTray } from "./file-tray";
import { GoldenThreadBar } from "./golden-thread";
import { HandoffPanel } from "./handoff-panel";
import { FileInsightsPanel } from "./insights-panel";
import type { PageToolCall } from "./page-edits";
import { PageGrid } from "./page-grid";
import { ToolResultCard } from "./result-card";
import { ToolInspector } from "./tool-inspector";
import { useToolLabel } from "./tool-label";
import type { RecipeStep, WorkspaceClient } from "./types";
import { useEditorWorkspace, type WorkspaceMode } from "./use-editor-workspace";
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
  /** Translated viewer chrome; English when omitted. */
  viewerLabels?: Partial<ViewerLabels>;
  /** Opens this tool; pass a new `nonce` to open the same tool again after it was changed. */
  requestedTool?: { id: string; nonce: number } | null;
  loadBackend?: ViewerBackendLoader;
  /** Largest total input a tool may read. */
  budgetBytes?: number;
  /** Called after a PDF result was opened in the editor; the workspace itself switches to Document mode. */
  onOpenInEditor?: (artifact: ToolExecutionArtifact) => void;
  onSaveRecipe?: (steps: RecipeStep[]) => void;
  /**
   * Receives a recipe the AI panel drafted from a sentence. The host must treat it as untrusted:
   * show it behind a confirmation and never run it unprompted.
   */
  onOpenRecipeDraft?: (recipe: {
    steps: Array<{ tool: string; params: Record<string, unknown> }>;
  }) => void;
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
  viewerLabels,
  requestedTool,
  loadBackend = loadEmbedPdfBackend,
  budgetBytes,
  onOpenInEditor,
  onSaveRecipe,
  onOpenRecipeDraft,
}: DocumentWorkspaceProps) {
  const copy = useWorkspaceCopy();
  const workspace = useWorkspace({ client, onProgress, budgetBytes });
  const { active, tool, run, openTool } = workspace;
  const requestedId = requestedTool?.id;
  const requestedNonce = requestedTool?.nonce;
  useEffect(() => {
    if (requestedId) openTool(requestedId);
  }, [requestedId, requestedNonce, openTool]);
  // The parameters the inspector holds, so the batch queue can run them on every file.
  const [inspectorParams, setInspectorParams] = useState<{
    params: Record<string, unknown>;
    valid: boolean;
  }>({ params: {}, valid: true });
  const onParamsChange = useCallback(
    (params: Record<string, unknown>, validation: ParamsValidation) =>
      setInspectorParams({ params, valid: validation.isValid }),
    [],
  );
  // `null` follows the file count: two or more files open the batch queue on their own.
  const [batchChoice, setBatchChoice] = useState<boolean | null>(null);
  const singleInput = tool?.inputs.arity === "one";
  const batchOn = Boolean(tool && singleInput && (batchChoice ?? workspace.files.length >= 2));
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

  // The four editing modes share one position in the document, so a switch keeps the page and,
  // where the modes can map it, the selected block or object.
  const [mode, setMode] = useState<WorkspaceMode>("view");
  const [location, setLocation] = useState<EditorLocationState>({ page: 1 });
  const [pageRequest, setPageRequest] = useState<{ page: number; nonce: number }>();
  const [panel, setPanel] = useState<"tools" | "ai">("tools");
  // The AI panel mounts on first use and then stays, so its answers survive a tab change.
  const [aiOpened, setAiOpened] = useState(false);
  const [exported, setExported] = useState<EditorExport | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [editorCall, setEditorCall] = useState<{
    tool: ToolDescriptor;
    params: Record<string, unknown>;
  } | null>(null);
  const activeKey = active?.key;
  // A different document restarts at page 1.
  useEffect(() => {
    setLocation({ page: 1 });
    setPageRequest(undefined);
    setExported(null);
    setExportError(null);
    setEditorCall(null);
  }, [activeKey]);
  const reportPage = useCallback(
    (page: number) => setLocation((previous) => (previous.page === page ? previous : { page })),
    [],
  );
  const reportLocation = useCallback(
    (next: EditorLocationState) =>
      setLocation((previous) =>
        next.page === previous.page && next.xPt === undefined && next.blockId === undefined
          ? previous
          : next,
      ),
    [],
  );
  const editor = useEditorWorkspace({ client, active, mode, run, budgetBytes });
  // Estimated objects carry `ir:<blockId>` ids that `edit.apply` cannot take, so no CLI hand-off.
  const pageObjectsEstimated =
    editor.objects.entry?.status === "ready" && editor.objects.entry.value.source === "ir";

  const exportDocument = async ({ format, irJson }: ExportRequest) => {
    if (!active) return;
    setExportError(null);
    try {
      setExported(
        await exportEditedIr(client, irJson, format, active.name.replace(/\.[^./\\]+$/, "")),
      );
    } catch (reason) {
      setExportError(
        typeof reason === "object" && reason !== null && "message" in reason
          ? String((reason as { message: unknown }).message)
          : String(reason),
      );
      throw reason;
    }
  };

  const applyPage = async (params: EditApplyParams) => {
    const descriptor = workspace.tools.find((entry) => entry.id === "edit.apply");
    if (!descriptor) return;
    const request = params as unknown as Record<string, unknown>;
    setEditorCall({ tool: descriptor, params: request });
    await workspace.runTool(descriptor, request);
  };

  const applyAnnotations = async (calls: EditorToolCall[]) => {
    const last = calls.at(-1);
    const descriptor = last && workspace.tools.find((entry) => entry.id === last.toolId);
    if (descriptor && last) {
      setEditorCall({
        tool: descriptor,
        params: last.params as unknown as Record<string, unknown>,
      });
    }
    const outcome = await workspace.applyCalls(
      calls.map((call) => ({
        toolId: call.toolId,
        params: call.params as unknown as Record<string, unknown>,
      })),
    );
    // The surface keeps its pending work when the call fails.
    if (!outcome.ok) throw new Error(copy.editor.applyFailed(outcome.message));
  };

  const openInEditor = async (artifact: ToolExecutionArtifact) => {
    onOpenInEditor?.(artifact);
    // Only a PDF can become the document the editor modes work on.
    if (artifact.format !== "pdf") return;
    await workspace.promote(artifact);
    setMode("document");
  };

  const jumpToPage = (page: number) => {
    setLocation({ page });
    setPageRequest((previous) => ({ page, nonce: (previous?.nonce ?? 0) + 1 }));
    setMode("view");
  };

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
          <>
            <WorkspaceModeSwitch mode={mode} onChange={setMode} />
            {editor.purgeError && (
              <p role="alert" className="text-sm text-destructive">
                {copy.editor.purgeFailed(editor.purgeError)}
              </p>
            )}
            {mode === "view" && (
              <Tabs defaultValue="preview">
                <TabsList>
                  <TabsTrigger value="preview">{copy.workspace.viewer}</TabsTrigger>
                  <TabsTrigger value="pages">{copy.workspace.pagesTab}</TabsTrigger>
                </TabsList>
                <TabsContent value="preview">
                  <PdfViewer
                    source={source}
                    loadBackend={loadBackend}
                    labels={viewerLabels}
                    initialPage={location.page}
                    pageRequest={pageRequest}
                    onPageChange={reportPage}
                    className="h-[70vh]"
                  />
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
            )}
            {mode === "annotate" && active && (
              <AnnotatePane
                key={active.key}
                docKey={active.key}
                source={source}
                backend={backend}
                data={editor.data}
                busy={workspace.busy !== null}
                location={location}
                onLocationChange={(next) => reportPage(next.page)}
                onApply={applyAnnotations}
              />
            )}
            {mode === "document" && active && (
              <DocumentPane
                key={active.key}
                docKey={active.key}
                locale={locale}
                ir={editor.ir}
                store={editor.store}
                redacted={editor.redacted.has(active.key)}
                location={location}
                onLocationChange={reportLocation}
                onExport={exportDocument}
              />
            )}
            {mode === "page" && active && (
              <PagePane
                key={active.key}
                docKey={active.key}
                source={source}
                backend={backend}
                fallbackPages={
                  editor.data.entry?.status === "ready" ? editor.data.entry.value.pages : []
                }
                fallbackPending={!editor.data.entry || editor.data.entry.status === "loading"}
                objects={editor.objects}
                pageSession={(model) => editor.pageSession(active.key, model)}
                location={location}
                onLocationChange={reportLocation}
                onApply={applyPage}
              />
            )}
            {exported && mode === "document" && (
              <div
                data-slot="editor-export"
                className="flex flex-wrap items-center gap-2 rounded-xl border p-3 text-sm"
              >
                <span role="status">{copy.editor.exported(exported.name)}</span>
                <Button
                  size="sm"
                  onClick={() =>
                    void downloadOutput({ kind: "memory", blob: exported.blob }, exported.name)
                  }
                >
                  <DownloadIcon aria-hidden="true" />
                  {copy.editor.download(exported.name)}
                </Button>
              </div>
            )}
            {exportError && mode === "document" && (
              <p role="alert" className="text-sm text-destructive">
                {copy.editor.exportFailed(exportError)}
              </p>
            )}
            {mode === "page" && workspace.error && editorCall && (
              <div className="flex flex-col gap-2">
                <p role="alert" className="text-sm text-destructive">
                  {copy.editor.applyFailed(workspace.error)}
                </p>
                {!pageObjectsEstimated && (
                  <HandoffPanel
                    tool={editorCall.tool}
                    params={editorCall.params}
                    inputNames={active ? [active.name] : []}
                    locale={locale}
                  />
                )}
              </div>
            )}
          </>
        ) : (
          <p className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
            {copy.workspace.noPdf}
          </p>
        )}
      </div>

      <div className="flex min-w-0 flex-col gap-4">
        {workspace.thread.length > 0 && <GoldenThreadBar steps={workspace.thread} />}
        <Tabs
          value={panel}
          onValueChange={(next) => {
            setPanel(next === "ai" ? "ai" : "tools");
            if (next === "ai") setAiOpened(true);
          }}
        >
          <TabsList aria-label={copy.editor.panelTabs}>
            <TabsTrigger value="tools" data-testid="inspector-tab-tools">
              {copy.editor.tabTools}
            </TabsTrigger>
            <TabsTrigger value="ai" data-testid="inspector-tab-ai">
              {copy.editor.tabAi}
            </TabsTrigger>
          </TabsList>
          <TabsContent value="tools" keepMounted className="flex flex-col gap-4">
            <ToolInspector
              tool={tool}
              files={workspace.files}
              pageCount={active?.pageCount ?? undefined}
              cloudApiUrl={cloudApiUrl}
              locale={locale}
              busy={workspace.busy === "run"}
              progress={workspace.progress}
              error={workspace.error}
              onRun={async (params, options) => {
                if (tool) await workspace.runTool(tool, params, options);
              }}
              onRunCloud={onRunCloud && tool ? (params) => onRunCloud(tool.id, params) : undefined}
              onParamsChange={onParamsChange}
              onCancel={workspace.cancel}
            />
            {tool && singleInput && workspace.files.length > 0 && (
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={batchOn}
                  disabled={workspace.busy !== null}
                  onChange={(event) => setBatchChoice(event.target.checked)}
                />
                {copy.batch.toggle}
              </label>
            )}
            {batchOn && tool && (
              <BatchSection
                tool={tool}
                params={inspectorParams.params}
                paramsValid={inspectorParams.valid}
                files={workspace.files}
                locale={locale}
                onRemoveFile={workspace.removeFile}
              />
            )}
          </TabsContent>
          <TabsContent value="ai" keepMounted>
            {aiOpened && (
              <AiPanel
                inputName={active?.name}
                pageCount={active?.pageCount ?? undefined}
                // A local model is only claimed once the browser has actually loaded one.
                localRuntime={false}
                locale={locale}
                onExecute={
                  cloudApiUrl && onRunCloud
                    ? async (request) => {
                        await onRunCloud(request.tool, request.params as Record<string, unknown>);
                        return undefined;
                      }
                    : undefined
                }
                onJumpToPage={jumpToPage}
                onOpenRecipeDraft={
                  onOpenRecipeDraft
                    ? (draft) => onOpenRecipeDraft({ steps: draft.steps })
                    : undefined
                }
              />
            )}
          </TabsContent>
        </Tabs>
        {run && (
          <ToolResultCard
            run={run}
            locale={locale}
            onContinue={workspace.promote}
            onUndo={workspace.undo}
            onOpenInEditor={openInEditor}
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
  const toolLabel = useToolLabel();
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
