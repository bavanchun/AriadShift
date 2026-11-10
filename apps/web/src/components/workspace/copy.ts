"use client";
// English strings of the workspace. Every component reads them through `useWorkspaceCopy`,
// so a locale page can supply translations through `WorkspaceCopyProvider` without the
// components knowing about the message catalogue.
import { createContext, createElement, type ReactNode, useContext, useMemo } from "react";
import { toolTitle } from "../../lib/tools";

const plural = (count: number, one: string, other: string) =>
  `${count} ${count === 1 ? one : other}`;

export const DEFAULT_WORKSPACE_COPY = {
  fileTray: {
    privacy: "On this device · 0 bytes uploaded",
    title: "Files",
    empty: "Drop PDFs or other files here to start. Nothing is uploaded.",
    choose: "Choose files",
    addMore: "Add files",
    dropTitle: "Drop files to add them",
    dropDescription: "They stay in this browser.",
    pages: (count: number) => plural(count, "page", "pages"),
    remove: (name: string) => `Remove ${name}`,
    moveUp: (name: string) => `Move ${name} up`,
    moveDown: (name: string) => `Move ${name} down`,
    use: (name: string) => `Work on ${name}`,
    active: "Open",
    storage: (kind: "opfs" | "memory"): string =>
      kind === "opfs" ? "Stored in browser file storage" : "Held in memory",
    reading: "Reading files…",
    list: "Files in this workspace",
    problem: (name: string) => `${name} could not be read.`,
  },
  insights: {
    title: "What is in this PDF",
    loading: "Reading the document on this device…",
    failed: "The document could not be analysed.",
    pages: (count: number) => plural(count, "page", "pages"),
    scanned: (count: number) => `${plural(count, "scanned page", "scanned pages")}`,
    blank: (count: number) => `${plural(count, "blank page", "blank pages")}`,
    dpi: (min: number | null, max: number | null) =>
      min === null || max === null
        ? "Image resolution unknown"
        : min === max
          ? `Images about ${min} DPI`
          : `Images about ${min}–${max} DPI`,
    lowDpi: (pages: readonly number[]) =>
      `Low resolution on ${plural(pages.length, "page", "pages")} (${pages.slice(0, 6).join(", ")}${pages.length > 6 ? "…" : ""})`,
    formFields: (count: number) => plural(count, "empty required field", "empty required fields"),
    signatures: (count: number) => plural(count, "signature", "signatures"),
    metadata: "Contains metadata",
    editHistory: "Has edit history",
    encrypted: "Encrypted",
    linearized: "Linearized",
    clean: "None found",
    suggested: "Suggested next steps",
    noSuggestions: "Nothing needs attention. Pick any tool to continue.",
    open: (tool: string) => `Open ${tool}`,
  },
  pageGrid: {
    title: "Pages",
    toolbar: "Page edits",
    undo: "Undo",
    redo: "Redo",
    reset: "Reset",
    pending: (count: number) => plural(count, "pending change", "pending changes"),
    selectAll: "Select all",
    clearSelection: "Clear selection",
    selectRange: "Select pages",
    rangePlaceholder: "e.g. 3-7",
    rotateLeft: "Rotate left",
    rotateRight: "Rotate right",
    deleteSelected: "Delete selected",
    restoreSelected: "Restore selected",
    apply: "Apply changes",
    hint: "Drag the grip, or focus it and use the arrow keys, to reorder. Shift-click selects a range.",
    dragHint: "Drag to reorder, or use the arrow keys",
    moved: (page: number, position: number) => `Page ${page} moved to position ${position}`,
    cannotDeleteAll: "A document needs at least one page.",
    cardPage: (page: number) => `Page ${page}`,
    cardSelect: (page: number) => `Select page ${page}`,
    cardRotate: (page: number) => `Rotate page ${page}`,
    cardDelete: (page: number) => `Delete page ${page}`,
    cardMove: (page: number) => `Reorder page ${page}`,
    cardRestore: (page: number) => `Restore page ${page}`,
    blank: "Blank",
    scanned: "Scanned",
  },
  inspector: {
    title: "Tool",
    empty: "Choose a tool to see its options.",
    runtime: { browser: "Browser", desktop: "Desktop", cloud: "Cloud" },
    tier: { free: "Free", pro: "Pro" },
    runHere: "Run tool on this device",
    running: "Running…",
    cancel: "Cancel",
    runCloud: "Run in Cloud",
    cloudNote: "This tool runs on our servers; your file is uploaded for it.",
    switchDesktop: "Run on your computer instead",
    switchBrowser: "Back to this device",
    needsFile: "Add a file first.",
    needsPdf: "This tool needs a PDF.",
    invalid: "Fix the highlighted options to continue.",
    failed: (message: string) => `The tool stopped: ${message}`,
    progress: "Tool progress",
  },
  handoff: {
    title: "Run on your computer",
    why: "This tool cannot run in the browser. Run it with the AriadShift command line; your files never leave your computer.",
    command: "Command",
    copy: "Copy command",
    copied: "Command copied",
    copyFailed: "Could not copy. Select the command and copy it.",
    install: "Get AriadShift",
    openApp: "Open in the desktop app",
    revealSecrets: "Show secret values",
    maskSecrets: "Hide secret values",
    secretNote: "Secret values are hidden in the command until you reveal them.",
  },
  result: {
    title: "Result",
    before: "Before",
    after: "After",
    size: "Size",
    pages: "Pages",
    duration: "Time",
    change: (percent: number) =>
      percent === 0 ? "no change" : `${percent > 0 ? "+" : "−"}${Math.abs(percent)}%`,
    bytes: (value: number) => `${value.toLocaleString("en")} bytes`,
    seconds: (ms: number) => (ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`),
    pagesValue: (count: number | null) => (count === null ? "—" : String(count)),
    warnings: "Warnings",
    details: "Details",
    download: (name: string) => `Download ${name}`,
    openInEditor: "Open in editor",
    continue: "Continue with another tool",
    saveRecipe: "Save as recipe",
    undo: "Undo",
    outputs: "Output files",
    downloadFailed: "The file could not be saved.",
  },
  thread: {
    label: "Steps",
    status: {
      pending: "Waiting",
      running: "Running",
      completed: "Done",
      skipped: "Skipped",
      failed: "Failed",
    },
  },
  tools: {
    /** The display name of a tool; the locale page supplies the translated titles. */
    title: (id: string) => toolTitle({ id }),
  },
  workspace: {
    title: "Workspace",
    lede: "Everything runs in this browser tab.",
    viewer: "Preview",
    pagesTab: "Pages",
    toolsTab: "Tool",
    noPdf: "Add a PDF to preview and organise its pages.",
    tools: "Tools",
    toolPicker: "Choose a tool",
    dismissError: "Dismiss",
    working: "Working…",
  },
};

export type WorkspaceCopy = typeof DEFAULT_WORKSPACE_COPY;
export type WorkspaceCopyOverrides = { [K in keyof WorkspaceCopy]?: Partial<WorkspaceCopy[K]> };

const CopyContext = createContext<WorkspaceCopy>(DEFAULT_WORKSPACE_COPY);

/** Merges translated sections over the English defaults, one level deep. */
export function mergeWorkspaceCopy(overrides: WorkspaceCopyOverrides = {}): WorkspaceCopy {
  const merged: Record<string, unknown> = { ...DEFAULT_WORKSPACE_COPY };
  for (const [section, value] of Object.entries(overrides)) {
    const base = (DEFAULT_WORKSPACE_COPY as Record<string, unknown>)[section];
    merged[section] = { ...(base as object), ...(value as object) };
  }
  return merged as WorkspaceCopy;
}

export function WorkspaceCopyProvider({
  copy,
  children,
}: {
  copy?: WorkspaceCopyOverrides;
  children: ReactNode;
}) {
  const value = useMemo(() => mergeWorkspaceCopy(copy), [copy]);
  return createElement(CopyContext.Provider, { value }, children);
}

export function useWorkspaceCopy(): WorkspaceCopy {
  return useContext(CopyContext);
}
