"use client";
// The /app page: converting one file, browsing every tool, and the PDF workspace. The converter
// is the landing view and stays mounted; the hub and the workspace load on first use so the
// route's first load does not carry the tool registry, the viewer or the PDFium module.
import { Button } from "@ariadshift/ui/components/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@ariadshift/ui/components/tabs";
import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { ConverterApp } from "../converter/converter-app";
import { useLazyModule } from "./use-lazy-module";

type View = "convert" | "tools" | "workspace";

const loadWorkspace = () => import("./workspace-host");
const loadTools = () => import("./tools-host");

const isView = (value: string | null): value is View =>
  value === "convert" || value === "tools" || value === "workspace";

function isPaletteShortcut(event: KeyboardEvent) {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "k"
  );
}

/** Keeps `?view=` and `?tool=` in the address bar, so a tool can be linked and reloaded. */
function writeLocation(view: View, toolId: string | null) {
  const url = new URL(window.location.href);
  if (view === "convert") url.searchParams.delete("view");
  else url.searchParams.set("view", view);
  if (view === "workspace" && toolId) url.searchParams.set("tool", toolId);
  else url.searchParams.delete("tool");
  window.history.replaceState(window.history.state, "", url);
}

export function AppShell() {
  const t = useTranslations("web.app");
  const params = useSearchParams();
  const linkedTool = params.get("tool");
  const linkedView = params.get("view");
  const [view, setView] = useState<View>(
    linkedTool ? "workspace" : isView(linkedView) ? linkedView : "convert",
  );
  const [requested, setRequested] = useState<{ id: string; nonce: number } | null>(
    linkedTool ? { id: linkedTool, nonce: 0 } : null,
  );
  const nonce = useRef(0);
  // Each of these turns true on first use and stays true, so state in the view survives a tab change.
  const [toolsWanted, setToolsWanted] = useState(view === "tools");
  const [paletteOnLoad, setPaletteOnLoad] = useState(false);
  const [workspaceWanted, setWorkspaceWanted] = useState(view === "workspace");
  const tools = useLazyModule(loadTools, toolsWanted);
  const workspace = useLazyModule(loadWorkspace, workspaceWanted);

  const show = useCallback((next: View, toolId: string | null = null) => {
    setView(next);
    if (next === "tools") setToolsWanted(true);
    if (next === "workspace") setWorkspaceWanted(true);
    writeLocation(next, toolId);
  }, []);

  const openTool = useCallback(
    (toolId: string) => {
      nonce.current += 1;
      setRequested({ id: toolId, nonce: nonce.current });
      show("workspace", toolId);
    },
    [show],
  );

  // ⌘K / Ctrl+K works from the converter too. Until the hub exists nothing else listens for it, so
  // this loads the hub with its palette open; afterwards the hub's own listener takes over.
  useEffect(() => {
    if (toolsWanted) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isPaletteShortcut(event)) return;
      event.preventDefault();
      setPaletteOnLoad(true);
      setToolsWanted(true);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toolsWanted]);

  return (
    <Tabs value={view} onValueChange={(next) => isView(next) && show(next, requested?.id ?? null)}>
      <TabsList aria-label={t("sections_label")}>
        <TabsTrigger value="convert" data-testid="tab-convert">
          {t("tab.convert")}
        </TabsTrigger>
        <TabsTrigger value="tools" data-testid="tab-tools">
          {t("tab.tools")}
        </TabsTrigger>
        <TabsTrigger value="workspace" data-testid="tab-workspace">
          {t("tab.workspace")}
        </TabsTrigger>
      </TabsList>
      <TabsContent value="convert" keepMounted className="text-base">
        <ConverterApp />
      </TabsContent>
      <TabsContent value="tools" keepMounted className="text-base">
        {toolsWanted && (
          <div className="flex flex-col gap-4">
            <p className="text-muted-foreground">{t("tools_lede")}</p>
            <Lazy
              kind="tools"
              state={tools.state}
              retry={tools.retry}
              render={(module) => (
                <module.default
                  onSelectTool={openTool}
                  defaultPaletteOpen={paletteOnLoad}
                  onOpenPalette={() => setPaletteOnLoad(false)}
                />
              )}
            />
          </div>
        )}
      </TabsContent>
      <TabsContent value="workspace" keepMounted className="text-base">
        {workspaceWanted && (
          <Lazy
            kind="workspace"
            state={workspace.state}
            retry={workspace.retry}
            render={(module) => <module.default requestedTool={requested} />}
          />
        )}
      </TabsContent>
    </Tabs>
  );
}

function Lazy<T>({
  kind,
  state,
  retry,
  render,
}: {
  kind: "tools" | "workspace";
  state: { status: "idle" | "loading" | "failed" } | { status: "ready"; module: T };
  retry: () => void;
  render: (module: T) => ReactNode;
}) {
  const t = useTranslations("web.app");
  if (state.status === "ready") return render(state.module);
  if (state.status === "failed") {
    return (
      <div role="alert" className="flex flex-wrap items-center gap-3 text-sm">
        <span>{t(`${kind}_failed`)}</span>
        <Button type="button" variant="outline" size="sm" onClick={retry}>
          {t("retry")}
        </Button>
      </div>
    );
  }
  return (
    <p role="status" className="text-sm text-muted-foreground">
      {t(`${kind}_loading`)}
    </p>
  );
}
