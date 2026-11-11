"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import {
  AlignCenterIcon,
  AlignJustifyIcon,
  AlignLeftIcon,
  AlignRightIcon,
  ArrowDownIcon,
  ArrowUpIcon,
  AsteriskIcon,
  BoldIcon,
  CodeIcon,
  CopyIcon,
  DownloadIcon,
  HistoryIcon,
  ImageIcon,
  ItalicIcon,
  LinkIcon,
  MinusIcon,
  PaletteIcon,
  Redo2Icon,
  SaveIcon,
  SigmaIcon,
  StrikethroughIcon,
  SubscriptIcon,
  SuperscriptIcon,
  TableIcon,
  Trash2Icon,
  UnderlineIcon,
  Undo2Icon,
} from "lucide-react";
import {
  type ComponentType,
  type KeyboardEvent,
  type ReactNode,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { bytesToBase64, contentAssetId } from "./asset-id";
import { type DocumentModeLabels, fillLabel } from "./document-mode-labels";
import type { DocumentEditSession, SessionSnapshot } from "./document-session";
import {
  type BlockTypeOption,
  blockTypeOption,
  findBlock,
  getInlines,
  TEXT_BLOCKS,
} from "./document-tree";
import { activeMarks, characterStyleAt, inlineLength, linkAt, type MarkKind } from "./inline-ops";
import type { ParagraphStyle } from "./mapping";
import { resolveParagraphStyle } from "./style-utils";
import type { AutosaveStatus } from "./use-version-history";

export interface DocumentToolbarProps {
  session: DocumentEditSession;
  snapshot: SessionSnapshot;
  labels: DocumentModeLabels;
  stylesOpen: boolean;
  historyOpen: boolean;
  linkOpen: boolean;
  onToggleLink: () => void;
  onToggleStyles: () => void;
  onToggleHistory: () => void;
  onSaveCheckpoint: () => void;
  onExport: () => void;
  autosave: { status: AutosaveStatus; lastSavedAt: string | null };
  locale?: string;
  announce?: (message: string) => void;
}

type IconComponent = ComponentType<{ "aria-hidden"?: boolean | "true" | "false" }>;

const MARK_BUTTONS: {
  kind: MarkKind;
  icon: IconComponent;
  label: keyof DocumentModeLabels;
  key?: string;
}[] = [
  { kind: "bold", icon: BoldIcon, label: "bold", key: "B" },
  { kind: "italic", icon: ItalicIcon, label: "italic", key: "I" },
  { kind: "underline", icon: UnderlineIcon, label: "underline", key: "U" },
  { kind: "strike", icon: StrikethroughIcon, label: "strikethrough" },
  { kind: "code", icon: CodeIcon, label: "code" },
  { kind: "subscript", icon: SubscriptIcon, label: "subscript" },
  { kind: "superscript", icon: SuperscriptIcon, label: "superscript" },
];

const ALIGNMENTS: {
  value: NonNullable<ParagraphStyle["align"]>;
  icon: IconComponent;
  label: keyof DocumentModeLabels;
}[] = [
  { value: "left", icon: AlignLeftIcon, label: "alignLeft" },
  { value: "center", icon: AlignCenterIcon, label: "alignCenter" },
  { value: "right", icon: AlignRightIcon, label: "alignRight" },
  { value: "justify", icon: AlignJustifyIcon, label: "alignJustify" },
];

const FONT_SUGGESTIONS = [
  "Noto Sans",
  "Noto Serif",
  "Libertinus Serif",
  "Times New Roman",
  "Arial",
  "Courier New",
];

const isMac = () => typeof navigator !== "undefined" && /mac/i.test(navigator.platform);

function ToolbarButton({
  label,
  icon: Icon,
  pressed,
  disabled,
  onClick,
  shortcut,
  children,
}: {
  label: string;
  icon: IconComponent;
  pressed?: boolean;
  disabled?: boolean;
  onClick: () => void;
  shortcut?: string;
  children?: ReactNode;
}) {
  const title = shortcut ? `${label} (${isMac() ? "⌘" : "Ctrl+"}${shortcut})` : label;
  return (
    <Button
      type="button"
      variant={pressed ? "secondary" : "ghost"}
      size={children ? "sm" : "icon-sm"}
      aria-label={label}
      aria-pressed={pressed}
      title={title}
      disabled={disabled}
      data-toolbar-item=""
      // Keep the text selection inside the editable while a toolbar button is pressed.
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      <Icon aria-hidden="true" />
      {children}
    </Button>
  );
}

/** Input that commits on Enter or blur, resyncing whenever the model value changes. */
function CommitInput({
  label,
  value,
  onCommit,
  className,
  type = "text",
  list,
  min,
  step,
  disabled,
}: {
  label: string;
  value: string;
  onCommit: (value: string) => void;
  className?: string;
  type?: "text" | "number";
  list?: string;
  min?: number;
  step?: number;
  disabled?: boolean;
}) {
  return (
    <input
      key={value}
      type={type}
      aria-label={label}
      title={label}
      defaultValue={value}
      list={list}
      min={min}
      step={step}
      disabled={disabled}
      data-toolbar-item=""
      className={`h-7 rounded-md border border-border bg-background px-2 text-sm ${className ?? ""}`}
      onBlur={(event) => {
        if (event.target.value !== value) onCommit(event.target.value);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          if (event.currentTarget.value !== value) onCommit(event.currentTarget.value);
        }
      }}
    />
  );
}

function Separator() {
  return <hr aria-orientation="vertical" className="mx-1 h-5 w-px border-0 bg-border" />;
}

export function DocumentToolbar({
  session,
  snapshot,
  labels,
  stylesOpen,
  historyOpen,
  linkOpen,
  onToggleLink,
  onToggleStyles,
  onToggleHistory,
  onSaveCheckpoint,
  onExport,
  autosave,
  locale,
  announce,
}: DocumentToolbarProps) {
  const root = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const { state, selection, selectedBlockId } = snapshot;
  const styles = state.original.styles;

  const selected = selectedBlockId ? findBlock(state.doc, selectedBlockId) : null;
  const target =
    selection ??
    (selectedBlockId
      ? { blockId: selectedBlockId, part: "content" as const, from: 0, to: 0 }
      : null);
  const targetBlock = target ? findBlock(state.doc, target.blockId) : null;
  const nodes = targetBlock && target ? getInlines(targetBlock.node, target.part) : null;
  const range =
    nodes && target
      ? target.from === target.to
        ? { from: 0, to: inlineLength(nodes) }
        : { from: target.from, to: target.to }
      : null;
  const active =
    nodes && range ? activeMarks(nodes, range.from, range.to, styles) : new Set<MarkKind>();
  const characterStyle =
    nodes && range ? characterStyleAt(nodes, range.from, range.to, styles) : {};
  const hasRange = Boolean(selection && selection.from !== selection.to);
  const paragraph = resolveParagraphStyle(
    styles,
    typeof selected?.node.attrs?.styleId === "string" ? selected.node.attrs.styleId : null,
    selected?.node.attrs?.direct as ParagraphStyle | undefined,
  );
  const isTextSelected = Boolean(selected && TEXT_BLOCKS.has(selected.node.type));
  const link = nodes && range ? linkAt(nodes, range.from, range.to) : null;
  const blockType = blockTypeOption(state.doc, selectedBlockId);
  const styleId =
    typeof selected?.node.attrs?.styleId === "string" ? selected.node.attrs.styleId : "";
  const namedStyles = Object.keys(styles?.paragraph ?? {});

  // One roving tab stop: arrows move between the toolbar's controls.
  useLayoutEffect(() => {
    const items = root.current
      ? Array.from(root.current.querySelectorAll<HTMLElement>("[data-toolbar-item]"))
      : [];
    const current = items.find(
      (item) => item.getAttribute("tabindex") === "0" && !item.hasAttribute("disabled"),
    );
    const stop = current ?? items.find((item) => !item.hasAttribute("disabled"));
    for (const item of items) item.setAttribute("tabindex", item === stop ? "0" : "-1");
  });

  const onToolbarKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const key = event.key;
    if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(key)) return;
    const element = event.target as HTMLElement;
    // Text fields keep their own arrow-key editing.
    if (element.tagName === "INPUT" && (key === "ArrowRight" || key === "ArrowLeft")) return;
    const items = Array.from(
      root.current?.querySelectorAll<HTMLElement>("[data-toolbar-item]") ?? [],
    ).filter((item) => !item.hasAttribute("disabled"));
    const index = items.indexOf(element);
    if (index < 0) return;
    event.preventDefault();
    const next =
      key === "Home"
        ? 0
        : key === "End"
          ? items.length - 1
          : (index + (key === "ArrowRight" ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  };

  const onFocusCapture = (event: React.FocusEvent<HTMLDivElement>) => {
    const items = root.current?.querySelectorAll<HTMLElement>("[data-toolbar-item]") ?? [];
    const focused = (event.target as HTMLElement).closest<HTMLElement>("[data-toolbar-item]");
    if (!focused) return;
    for (const item of items) item.setAttribute("tabindex", item === focused ? "0" : "-1");
  };

  const insertBlock = (spec: Parameters<DocumentEditSession["insertBlock"]>[0]) => {
    if (session.insertBlock(spec)) announce?.(labels.announceInserted);
  };

  const insertImage = async (file: File | undefined) => {
    if (!file?.type.startsWith("image/")) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const id = await contentAssetId(bytes);
    if (session.insertImage({ id, base64: bytesToBase64(bytes), mediaType: file.type })) {
      announce?.(labels.announceInserted);
    }
  };

  const applyFontFamily = (family: string) => {
    const font = family.trim() ? { family: family.trim() } : undefined;
    if (hasRange) session.applyCharacterDirect({ font });
    else session.setBlockDirect({ font });
  };

  const applyFontSize = (value: string) => {
    const size = Number(value);
    const sizePt = value.trim() && Number.isFinite(size) && size > 0 ? size : undefined;
    if (hasRange) session.applyCharacterDirect({ size_pt: sizePt });
    else session.setBlockDirect({ size_pt: sizePt });
  };

  const statusText = (): string => {
    switch (autosave.status) {
      case "pending":
        return labels.autosavePending;
      case "saving":
        return labels.autosaveSaving;
      case "saved":
        return fillLabel(labels.autosaveSaved, {
          time: autosave.lastSavedAt
            ? new Date(autosave.lastSavedAt).toLocaleTimeString(locale)
            : "",
        });
      case "error":
        return labels.autosaveFailed;
      case "off":
        return labels.autosaveOff;
      case "purged":
        return labels.autosaveRedacted;
      default:
        return labels.autosaveIdle;
    }
  };

  const blockOptions: { value: BlockTypeOption; label: string }[] = [
    { value: "paragraph", label: labels.paragraph },
    ...([1, 2, 3, 4, 5, 6] as const).map((level) => ({
      value: `heading-${level}` as BlockTypeOption,
      label: fillLabel(labels.heading, { level }),
    })),
    { value: "quote", label: labels.quote },
    { value: "code", label: labels.codeBlock },
    { value: "bulletList", label: labels.bulletList },
    { value: "orderedList", label: labels.orderedList },
  ];

  return (
    <div className="flex flex-col gap-2 border-b border-border bg-card px-2 py-1.5">
      <div
        ref={root}
        role="toolbar"
        aria-label={labels.toolbarLabel}
        aria-orientation="horizontal"
        className="flex flex-wrap items-center gap-1"
        onKeyDown={onToolbarKeyDown}
        onFocusCapture={onFocusCapture}
      >
        <ToolbarButton
          label={labels.undo}
          icon={Undo2Icon}
          shortcut="Z"
          disabled={!snapshot.canUndo}
          onClick={() => session.undo()}
        />
        <ToolbarButton
          label={labels.redo}
          icon={Redo2Icon}
          shortcut="Shift+Z"
          disabled={!snapshot.canRedo}
          onClick={() => session.redo()}
        />
        <Separator />
        <select
          aria-label={labels.blockType}
          title={labels.blockType}
          data-toolbar-item=""
          value={blockType ?? ""}
          disabled={!blockType}
          className="h-7 rounded-md border border-border bg-background px-2 text-sm"
          onChange={(event) => session.setBlockType(event.target.value as BlockTypeOption)}
        >
          {!blockType && <option value="">{labels.blockType}</option>}
          {blockOptions.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <select
          aria-label={labels.namedStyle}
          title={labels.namedStyle}
          data-toolbar-item=""
          value={styleId}
          disabled={!isTextSelected}
          className="h-7 rounded-md border border-border bg-background px-2 text-sm"
          onChange={(event) => session.setBlockStyle(event.target.value || null)}
        >
          <option value="">{labels.noNamedStyle}</option>
          {namedStyles.map((id) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
        </select>
        <CommitInput
          label={labels.fontFamily}
          value={characterStyle.font?.family ?? paragraph.font?.family ?? ""}
          list="document-font-suggestions"
          className="w-32"
          disabled={!isTextSelected}
          onCommit={applyFontFamily}
        />
        <datalist id="document-font-suggestions">
          {FONT_SUGGESTIONS.map((font) => (
            <option key={font} value={font} />
          ))}
        </datalist>
        <CommitInput
          label={labels.fontSize}
          type="number"
          min={1}
          step={0.5}
          value={String(characterStyle.size_pt ?? paragraph.size_pt ?? "")}
          className="w-16"
          disabled={!isTextSelected}
          onCommit={applyFontSize}
        />
        <Separator />
        {MARK_BUTTONS.map(({ kind, icon, label, key }) => (
          <ToolbarButton
            key={kind}
            label={labels[label]}
            icon={icon}
            shortcut={key}
            pressed={active.has(kind)}
            disabled={!nodes}
            onClick={() => session.toggleMark(kind)}
          />
        ))}
        <ToolbarButton
          label={labels.link}
          icon={LinkIcon}
          shortcut="K"
          pressed={active.has("link") || linkOpen}
          disabled={!nodes}
          onClick={onToggleLink}
        />
        <Separator />
        {ALIGNMENTS.map(({ value, icon, label }) => (
          <ToolbarButton
            key={value}
            label={labels[label]}
            icon={icon}
            pressed={(paragraph.align ?? "left") === value}
            disabled={!isTextSelected}
            onClick={() => session.setBlockDirect({ align: value })}
          />
        ))}
        <Separator />
        <ToolbarButton
          label={labels.insertTable}
          icon={TableIcon}
          onClick={() => insertBlock({ kind: "table", rows: 3, columns: 3, header: true })}
        />
        <ToolbarButton
          label={labels.insertImage}
          icon={ImageIcon}
          onClick={() => fileInput.current?.click()}
        />
        <input
          ref={fileInput}
          type="file"
          accept="image/*"
          hidden
          aria-hidden="true"
          tabIndex={-1}
          data-testid="insert-image-input"
          onChange={(event) => {
            void insertImage(event.target.files?.[0]);
            event.target.value = "";
          }}
        />
        <ToolbarButton
          label={labels.insertMath}
          icon={SigmaIcon}
          onClick={() => insertBlock({ kind: "mathBlock", tex: "" })}
        />
        <ToolbarButton
          label={labels.insertRule}
          icon={MinusIcon}
          onClick={() => insertBlock({ kind: "horizontalRule" })}
        />
        <ToolbarButton
          label={labels.insertFootnote}
          icon={AsteriskIcon}
          disabled={!isTextSelected && !selection}
          onClick={() => {
            if (session.insertFootnote()) announce?.(labels.announceInserted);
          }}
        />
        <Separator />
        {/* biome-ignore lint/a11y/useSemanticElements: a labelled group of toolbar buttons */}
        <div role="group" aria-label={labels.blockActions} className="flex items-center gap-1">
          <ToolbarButton
            label={labels.moveUp}
            icon={ArrowUpIcon}
            disabled={!selectedBlockId}
            onClick={() => session.moveBlock(-1) && announce?.(labels.announceMovedUp)}
          />
          <ToolbarButton
            label={labels.moveDown}
            icon={ArrowDownIcon}
            disabled={!selectedBlockId}
            onClick={() => session.moveBlock(1) && announce?.(labels.announceMovedDown)}
          />
          <ToolbarButton
            label={labels.duplicateBlock}
            icon={CopyIcon}
            disabled={!selectedBlockId}
            onClick={() => session.duplicateBlock() && announce?.(labels.announceDuplicated)}
          />
          <ToolbarButton
            label={labels.deleteBlock}
            icon={Trash2Icon}
            disabled={!selectedBlockId}
            onClick={() => session.deleteBlock() && announce?.(labels.announceDeleted)}
          />
        </div>
        <div className="ms-auto flex items-center gap-1">
          <Badge variant="outline" data-testid="autosave-status" role="status">
            {statusText()}
          </Badge>
          <ToolbarButton
            label={labels.stylesPanel}
            icon={PaletteIcon}
            pressed={stylesOpen}
            onClick={onToggleStyles}
          />
          <ToolbarButton
            label={labels.versionHistory}
            icon={HistoryIcon}
            pressed={historyOpen}
            onClick={onToggleHistory}
          />
          <ToolbarButton label={labels.saveCheckpoint} icon={SaveIcon} onClick={onSaveCheckpoint} />
          <ToolbarButton label={labels.export} icon={DownloadIcon} onClick={onExport}>
            {labels.export}
          </ToolbarButton>
        </div>
      </div>
      {linkOpen && (
        // biome-ignore lint/a11y/useSemanticElements: a labelled group, not a form
        <div role="group" aria-label={labels.link} className="flex flex-wrap items-center gap-2">
          <LinkForm
            key={`${link?.href ?? ""}|${link?.title ?? ""}`}
            labels={labels}
            initial={link}
            onApply={(href, title) => {
              session.setLink(href, title || null);
              onToggleLink();
            }}
            onRemove={() => {
              session.removeLink();
              onToggleLink();
            }}
          />
        </div>
      )}
    </div>
  );
}

function LinkForm({
  labels,
  initial,
  onApply,
  onRemove,
}: {
  labels: DocumentModeLabels;
  initial: { href: string; title: string | null } | null;
  onApply: (href: string, title: string) => void;
  onRemove: () => void;
}) {
  const [href, setHref] = useState(initial?.href ?? "");
  const [title, setTitle] = useState(initial?.title ?? "");
  return (
    <>
      <input
        type="url"
        aria-label={labels.linkUrl}
        placeholder="https://"
        value={href}
        className="h-7 w-64 rounded-md border border-border bg-background px-2 text-sm"
        onChange={(event) => setHref(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            onApply(href, title);
          }
        }}
      />
      <input
        type="text"
        aria-label={labels.linkTitle}
        placeholder={labels.linkTitle}
        value={title}
        className="h-7 w-40 rounded-md border border-border bg-background px-2 text-sm"
        onChange={(event) => setTitle(event.target.value)}
      />
      <Button type="button" size="sm" onClick={() => onApply(href, title)}>
        {labels.applyLink}
      </Button>
      <Button type="button" size="sm" variant="outline" onClick={onRemove}>
        {labels.removeLink}
      </Button>
    </>
  );
}
