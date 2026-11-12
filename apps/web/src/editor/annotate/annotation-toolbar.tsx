"use client";

import { Button } from "@ariadshift/ui/components/button";
import {
  ArrowUpRightIcon,
  HighlighterIcon,
  MessageSquareIcon,
  MousePointer2Icon,
  PencilLineIcon,
  RedoIcon,
  ShapesIcon,
  StampIcon,
  StrikethroughIcon,
  UnderlineIcon,
  Undo2Icon,
} from "lucide-react";
import type { ComponentType, KeyboardEvent } from "react";
import { useStore } from "zustand";
import {
  type AnnotationStore,
  type AnnotationTool,
  HIGHLIGHT_COLOURS,
  NOTE_ICONS,
  pendingCount,
  SHAPE_KINDS,
  STAMP_PRESETS,
} from "./annotation-store";
import { useAnnotateCopy } from "./copy";

interface ToolEntry {
  tool: AnnotationTool;
  key: string;
  Icon: ComponentType<{ "aria-hidden"?: "true" }>;
}

const TOOLS: readonly ToolEntry[] = [
  { tool: "select", key: "v", Icon: MousePointer2Icon },
  { tool: "highlight", key: "h", Icon: HighlighterIcon },
  { tool: "underline", key: "u", Icon: UnderlineIcon },
  { tool: "strike", key: "s", Icon: StrikethroughIcon },
  { tool: "note", key: "n", Icon: MessageSquareIcon },
  { tool: "ink", key: "i", Icon: PencilLineIcon },
  { tool: "shape", key: "r", Icon: ShapesIcon },
  { tool: "stamp", key: "t", Icon: StampIcon },
];

const isTyping = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));

/**
 * Keyboard shortcuts of the Annotate mode: one letter per tool, and undo/redo with the usual
 * modifier chords. Letters are ignored while typing in a field. Returns whether it handled the key.
 */
export function handleAnnotationShortcut(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey" | "target"> & {
    preventDefault(): void;
  },
  store: AnnotationStore,
): boolean {
  if (isTyping(event.target) || event.altKey) return false;
  const key = event.key.toLowerCase();
  if (event.ctrlKey || event.metaKey) {
    if (key === "z") {
      event.preventDefault();
      if (event.shiftKey) store.getState().redo();
      else store.getState().undo();
      return true;
    }
    if (key === "y") {
      event.preventDefault();
      store.getState().redo();
      return true;
    }
    return false;
  }
  const entry = TOOLS.find((candidate) => candidate.key === key);
  if (!entry) return false;
  event.preventDefault();
  store.getState().setTool(entry.tool);
  return true;
}

export interface AnnotationToolbarProps {
  store: AnnotationStore;
  /** Places the current tool's annotation at the centre of the page, for keyboard users. */
  onPlaceAtCentre?: () => void;
}

export function AnnotationToolbar({ store, onPlaceAtCentre }: AnnotationToolbarProps) {
  const copy = useAnnotateCopy();
  const labels = copy.toolbar;
  const state = useStore(store);
  const { settings } = state;
  const { tool } = settings;
  const drawing = tool !== "select";
  const usesWidth = tool === "ink" || tool === "shape";

  return (
    <div data-slot="annotation-toolbar" className="flex min-w-0 flex-col gap-3">
      <div role="toolbar" aria-label={labels.label} className="flex flex-wrap items-center gap-1.5">
        {TOOLS.map(({ tool: entry, key, Icon }) => (
          <Button
            key={entry}
            type="button"
            variant={tool === entry ? "default" : "outline"}
            size="sm"
            aria-pressed={tool === entry}
            aria-keyshortcuts={key.toUpperCase()}
            title={labels.shortcut(labels.tools[entry], key.toUpperCase())}
            onClick={() => state.setTool(entry)}
          >
            <Icon aria-hidden="true" />
            {labels.tools[entry]}
          </Button>
        ))}
        <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={state.past.length === 0}
          aria-keyshortcuts="Control+Z Meta+Z"
          onClick={state.undo}
        >
          <Undo2Icon aria-hidden="true" />
          {labels.undo}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={state.future.length === 0}
          aria-keyshortcuts="Control+Shift+Z Meta+Shift+Z"
          onClick={state.redo}
        >
          <RedoIcon aria-hidden="true" />
          {labels.redo}
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={pendingCount(state) === 0 && state.past.length === 0}
          onClick={state.reset}
        >
          {labels.reset}
        </Button>
      </div>

      {drawing ? (
        <fieldset
          aria-label={labels.tools[tool]}
          className="flex min-w-0 flex-wrap items-end gap-x-4 gap-y-2 rounded-lg border p-2"
        >
          <fieldset
            aria-label={labels.colour}
            className="m-0 min-w-0 border-0 p-0 flex items-center gap-1"
          >
            {(Object.entries(HIGHLIGHT_COLOURS) as [keyof typeof HIGHLIGHT_COLOURS, string][]).map(
              ([name, hex]) => (
                <button
                  key={name}
                  type="button"
                  aria-label={labels.swatch(labels.swatches[name])}
                  aria-pressed={settings.color.toLowerCase() === hex.toLowerCase()}
                  onClick={() => state.configure({ color: hex })}
                  className="size-6 rounded-full border-2 border-transparent outline-none focus-visible:ring-3 focus-visible:ring-ring/50 aria-pressed:border-foreground"
                  style={{ backgroundColor: hex }}
                />
              ),
            )}
            <input
              type="color"
              aria-label={labels.customColour}
              value={settings.color}
              onChange={(event) => state.configure({ color: event.currentTarget.value })}
              className="size-6 cursor-pointer rounded border bg-transparent p-0"
            />
          </fieldset>

          <label className="flex items-center gap-2 text-xs">
            {labels.opacity}
            <input
              type="range"
              min={0.1}
              max={1}
              step={0.05}
              value={settings.opacity}
              aria-label={labels.opacity}
              onChange={(event) => state.configure({ opacity: Number(event.currentTarget.value) })}
            />
          </label>

          {usesWidth ? (
            <label className="flex items-center gap-2 text-xs">
              {labels.strokeWidth}
              <input
                type="range"
                min={1}
                max={12}
                step={1}
                value={settings.lineWidth}
                aria-label={labels.strokeWidth}
                onChange={(event) =>
                  state.configure({ lineWidth: Number(event.currentTarget.value) })
                }
              />
            </label>
          ) : null}

          {tool === "shape" ? (
            <>
              <fieldset
                aria-label={labels.shape}
                className="m-0 min-w-0 border-0 p-0 flex items-center gap-1"
              >
                {SHAPE_KINDS.map((shape) => (
                  <Button
                    key={shape}
                    type="button"
                    size="xs"
                    variant={settings.shape === shape ? "default" : "outline"}
                    aria-pressed={settings.shape === shape}
                    onClick={() => state.configure({ shape })}
                  >
                    {shape === "arrow" ? <ArrowUpRightIcon aria-hidden="true" /> : null}
                    {labels.shapes[shape]}
                  </Button>
                ))}
              </fieldset>
              <fieldset
                aria-label={labels.fillColour}
                className="m-0 min-w-0 border-0 p-0 flex items-center gap-1"
              >
                <Button
                  type="button"
                  size="xs"
                  variant={settings.fill === null ? "default" : "outline"}
                  aria-pressed={settings.fill === null}
                  onClick={() => state.configure({ fill: null })}
                >
                  {labels.noFill}
                </Button>
                <input
                  type="color"
                  aria-label={labels.fillColour}
                  value={settings.fill ?? "#ffffff"}
                  onChange={(event) => state.configure({ fill: event.currentTarget.value })}
                  className="size-6 cursor-pointer rounded border bg-transparent p-0"
                />
              </fieldset>
            </>
          ) : null}

          {tool === "stamp" ? (
            <fieldset
              aria-label={labels.stamp}
              className="m-0 flex min-w-0 flex-wrap items-center gap-1 border-0 p-0"
            >
              {STAMP_PRESETS.map((preset) => (
                <Button
                  key={preset}
                  type="button"
                  size="xs"
                  variant={settings.stamp === preset ? "default" : "outline"}
                  aria-pressed={settings.stamp === preset}
                  onClick={() => state.configure({ stamp: preset })}
                >
                  {preset}
                </Button>
              ))}
              <input
                type="text"
                aria-label={labels.customStamp}
                placeholder={labels.customStamp}
                value={
                  STAMP_PRESETS.includes(settings.stamp as (typeof STAMP_PRESETS)[number])
                    ? ""
                    : settings.stamp
                }
                onChange={(event) => {
                  const text = event.currentTarget.value;
                  state.configure({ stamp: text === "" ? STAMP_PRESETS[0] : text });
                }}
                className="h-6 rounded-md border bg-background px-2 text-xs"
              />
            </fieldset>
          ) : null}

          {tool === "note" ? (
            <>
              <label className="flex items-center gap-2 text-xs">
                {labels.author}
                <input
                  type="text"
                  value={settings.author}
                  onChange={(event) => state.configure({ author: event.currentTarget.value })}
                  className="h-6 rounded-md border bg-background px-2 text-xs"
                />
              </label>
              <label className="flex items-center gap-2 text-xs">
                {labels.icon}
                <select
                  value={settings.icon}
                  onChange={(event) =>
                    state.configure({
                      icon: event.currentTarget.value as (typeof NOTE_ICONS)[number],
                    })
                  }
                  className="h-6 rounded-md border bg-background px-1 text-xs"
                >
                  {NOTE_ICONS.map((icon) => (
                    <option key={icon} value={icon}>
                      {labels.icons[icon]}
                    </option>
                  ))}
                </select>
              </label>
            </>
          ) : null}

          <div className="ms-auto flex items-center gap-2">
            <span className="text-xs text-muted-foreground">{labels.placeHint}</span>
            {onPlaceAtCentre ? (
              <Button type="button" size="xs" variant="secondary" onClick={onPlaceAtCentre}>
                {labels.place(labels.tools[tool])}
              </Button>
            ) : null}
          </div>
        </fieldset>
      ) : null}
    </div>
  );
}
