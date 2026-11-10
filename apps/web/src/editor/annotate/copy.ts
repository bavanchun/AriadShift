"use client";
// English strings of the Annotate surface. Components read them through `useAnnotateCopy`, so
// a locale page can supply translations through `AnnotateCopyProvider` without the components
// knowing about the message catalogue.
import { createContext, createElement, type ReactNode, useContext } from "react";
import type { Annotation, AnnotationKind, NoteIcon, ShapeKind } from "./annotation-store";
import type { DesignFieldType, DesignIssue } from "./form-model";

const plural = (count: number, one: string, other: string) =>
  `${count} ${count === 1 ? one : other}`;

export const DEFAULT_ANNOTATE_COPY = {
  surface: {
    title: "Annotate and forms",
    modes: "Mode",
    annotate: "Annotate",
    fill: "Fill form",
    design: "Design form",
    page: "Page",
    pageOf: (page: number, total: number) => `Page ${page} of ${total}`,
    previousPage: "Previous page",
    nextPage: "Next page",
    canvas: (page: number) => `Page ${page} drawing area`,
    apply: "Apply changes",
    applying: "Applying…",
    failed: "The changes could not be applied.",
  },
  toolbar: {
    label: "Annotation tools",
    undo: "Undo",
    redo: "Redo",
    reset: "Discard all",
    tools: {
      select: "Select",
      highlight: "Highlight",
      underline: "Underline",
      strike: "Strikethrough",
      note: "Sticky note",
      ink: "Freehand ink",
      shape: "Shapes",
      stamp: "Stamp",
    } satisfies Record<AnnotationKind | "select", string>,
    shortcut: (label: string, key: string) => `${label} (${key})`,
    colour: "Colour",
    swatch: (name: string) => `${name} colour`,
    swatches: {
      yellow: "Yellow",
      green: "Green",
      blue: "Blue",
      pink: "Pink",
      orange: "Orange",
    },
    customColour: "Custom colour",
    opacity: "Opacity",
    strokeWidth: "Stroke width",
    shape: "Shape",
    shapes: {
      rectangle: "Rectangle",
      ellipse: "Ellipse",
      line: "Line",
      arrow: "Arrow",
    } satisfies Record<ShapeKind, string>,
    fillColour: "Fill colour",
    noFill: "No fill",
    stamp: "Stamp",
    customStamp: "Custom stamp text",
    author: "Author",
    icon: "Note icon",
    icons: {
      comment: "Comment",
      note: "Note",
      key: "Key",
      help: "Help",
      insert: "Insert",
    } satisfies Record<NoteIcon, string>,
    placeHint: "Drag on the page to place it, or use the button to place it at the centre.",
    place: (tool: string) => `Place ${tool.toLowerCase()} at centre`,
  },
  list: {
    title: "Annotations",
    empty: "No annotations yet.",
    pending: (count: number) => plural(count, "pending change", "pending changes"),
    item: (kind: string, page: number) => `${kind} on page ${page}`,
    existing: (type: string, page: number) => `${type} on page ${page} (in the PDF)`,
    remove: (label: string) => `Delete ${label}`,
    restore: (label: string) => `Keep ${label}`,
    removed: "Will be deleted",
    comment: "Comment",
    stampText: "Stamp text",
    delete: "Delete",
    nudge: "Move with the arrow keys",
  },
  fill: {
    title: "Fill form",
    empty: "This PDF has no fillable form fields.",
    required: "Required",
    remaining: (count: number) =>
      count === 0
        ? "All required fields are filled"
        : `${plural(count, "required field", "required fields")} remaining`,
    next: "Go to next required field",
    flatten: "Flatten after filling",
    flattenHint: "Bakes the answers into the page so they can no longer be edited.",
    signature: "Add this signature with the Sign tool.",
    readOnly: "Read only",
    choose: "Choose…",
    page: (page: number) => `Page ${page}`,
    marker: (name: string, order: number) => `Field ${order}: ${name}`,
    list: "Form fields in tab order",
    reset: "Reset answers",
  },
  design: {
    title: "Design form",
    empty: "Add a field to start designing a fillable form.",
    add: "Add field",
    addType: (type: string) => `Add ${type.toLowerCase()} field`,
    types: {
      text: "Text",
      checkbox: "Checkbox",
      dropdown: "Dropdown",
      signature: "Signature",
    } satisfies Record<DesignFieldType, string>,
    list: "Fields in tab order",
    tabOrder: (order: number) => `Tab order ${order}`,
    moveUp: (name: string) => `Move ${name} up`,
    moveDown: (name: string) => `Move ${name} down`,
    remove: (name: string) => `Remove ${name}`,
    select: (name: string) => `Edit ${name}`,
    name: "Field name",
    label: "Label",
    defaultValue: "Default value",
    checked: "Checked by default",
    required: "Required",
    multiline: "Multiple lines",
    options: "Choices (one per line)",
    page: "Page",
    box: "Position and size",
    left: "Left",
    bottom: "Bottom",
    right: "Right",
    top: "Top",
    issues: {
      "name-empty": "Give the field a name.",
      "name-invalid": "A field name cannot contain a dot.",
      "name-duplicate": "Another field already uses this name.",
      rect: "The box must have a positive width and height.",
      default: "The default must be one of the choices.",
    } satisfies Record<DesignIssue, string>,
    create: "Create fields",
    fieldCount: (count: number) => plural(count, "field", "fields"),
  },
};

export type AnnotateCopy = typeof DEFAULT_ANNOTATE_COPY;

const AnnotateCopyContext = createContext<AnnotateCopy>(DEFAULT_ANNOTATE_COPY);

export function AnnotateCopyProvider({
  copy,
  children,
}: {
  copy: AnnotateCopy;
  children: ReactNode;
}) {
  return createElement(AnnotateCopyContext.Provider, { value: copy }, children);
}

export const useAnnotateCopy = (): AnnotateCopy => useContext(AnnotateCopyContext);

/** The accessible name of a placed annotation: its tool, then its 1-based position in the list. */
export const annotationLabel = (
  copy: AnnotateCopy,
  annotation: Pick<Annotation, "kind">,
  index: number,
): string => `${copy.toolbar.tools[annotation.kind]} ${index + 1}`;
