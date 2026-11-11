import { createContext, type MutableRefObject, useContext } from "react";
import type { DocumentModeLabels } from "./document-mode-labels";
import type { DocumentEditSession } from "./document-session";
import type { InlinePart } from "./document-tree";
import type { AssetRef, IrDocument, StyleSheet, TiptapNode } from "./mapping";

/** What to focus after the next render: the session's caret, or an explicit editable. */
export type FocusRequest = "selection" | { blockId: string; part: InlinePart };

export interface ActiveAtom {
  blockId: string;
  part: InlinePart;
  offset: number;
  node: TiptapNode;
}

export interface EditorContextValue {
  session: DocumentEditSession;
  styles: StyleSheet | undefined;
  assets: IrDocument["assets"];
  labels: DocumentModeLabels;
  focusRequest: MutableRefObject<FocusRequest | null>;
  activateAtom: (atom: ActiveAtom) => void;
  announce: (message: string) => void;
}

export const EditorContext = createContext<EditorContextValue | null>(null);

export function useEditorContext(): EditorContextValue {
  const value = useContext(EditorContext);
  if (!value) throw new Error("Document editor components must render inside DocumentModeEditor");
  return value;
}

const SAFE_PROTOCOLS = new Set(["http:", "https:", "mailto:", "tel:"]);

/** Only navigational schemes and relative references are rendered as links. */
export function safeHref(href: unknown): string | undefined {
  if (typeof href !== "string" || !href.trim()) return undefined;
  try {
    const url = new URL(href, "https://relative.invalid/");
    if (url.origin === "https://relative.invalid") return href;
    return SAFE_PROTOCOLS.has(url.protocol) ? href : undefined;
  } catch {
    return undefined;
  }
}

/**
 * A displayable `src` for an embedded IR asset. External image URLs are never loaded: fetching
 * them would send a request from the editor and break the "on this device" guarantee.
 */
export function imageSource(
  assets: IrDocument["assets"],
  target: AssetRef | undefined,
): string | null {
  if (target?.type !== "asset") return null;
  const asset = assets?.[target.id] as { bytes?: unknown; media_type?: unknown } | undefined;
  return typeof asset?.bytes === "string" &&
    typeof asset.media_type === "string" &&
    asset.media_type.startsWith("image/")
    ? `data:${asset.media_type};base64,${asset.bytes}`
    : null;
}
