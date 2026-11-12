"use client";

import { SearchIcon } from "lucide-react";
import * as React from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "#components/dialog";
import { cn } from "#lib/utils";

/** Scores how well `search` matches `value` plus `keywords`: 0 hides the item, higher ranks first. */
export type CommandFilter = (value: string, search: string, keywords: string[]) => number;

/** Default filter: every search word must appear in the value or keywords, case-insensitive. */
export const defaultCommandFilter: CommandFilter = (value, search, keywords) => {
  const words = search.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return 1;
  }
  const haystack = [value, ...keywords].join(" ").toLowerCase();
  return words.every((word) => haystack.includes(word)) ? 1 : 0;
};

interface ItemRecord {
  value: string;
  keywords: string[];
  disabled: boolean;
  groupId: string | undefined;
}

interface CommandContextValue {
  search: string;
  setSearch: (search: string) => void;
  activeId: string | undefined;
  setActiveId: (id: string) => void;
  listId: string;
  inputId: string;
  /** True for an item that has registered and does not match the search. */
  isHidden: (id: string) => boolean;
  visibleCount: number;
  visibleGroups: ReadonlySet<string>;
  register: (id: string, record: ItemRecord) => () => void;
  select: (id: string) => void;
  /** Select callbacks live in a ref so rendering an item never re-registers it. */
  handlers: React.RefObject<Map<string, () => void>>;
}

const CommandContext = React.createContext<CommandContextValue | null>(null);
const GroupContext = React.createContext<string | undefined>(undefined);

function useCommand() {
  const context = React.useContext(CommandContext);
  if (!context) {
    throw new Error("Command parts must be rendered inside <Command>.");
  }
  return context;
}

export interface CommandProps extends Omit<React.ComponentProps<"div">, "onChange"> {
  /** Controlled search text. */
  search?: string;
  defaultSearch?: string;
  onSearchChange?: (search: string) => void;
  /** Custom matching. Return 0 to hide an item. */
  filter?: CommandFilter;
  /** Wrap from the last item to the first and back with the arrow keys. */
  loop?: boolean;
}

/** Searchable list with keyboard navigation (↑ ↓ Home End Enter). Filtering is local; no network. */
function Command({
  className,
  search: searchProp,
  defaultSearch = "",
  onSearchChange,
  filter = defaultCommandFilter,
  loop = false,
  onKeyDown,
  ...props
}: CommandProps) {
  const listId = React.useId();
  const inputId = React.useId();
  const rootRef = React.useRef<HTMLDivElement>(null);
  const [innerSearch, setInnerSearch] = React.useState(defaultSearch);
  const search = searchProp ?? innerSearch;
  const [items, setItems] = React.useState<ReadonlyMap<string, ItemRecord>>(new Map());
  const [activeId, setActiveId] = React.useState<string | undefined>();
  const selectHandlers = React.useRef(new Map<string, () => void>());

  const setSearch = React.useCallback(
    (next: string) => {
      setInnerSearch(next);
      onSearchChange?.(next);
    },
    [onSearchChange],
  );

  const register = React.useCallback((id: string, record: ItemRecord) => {
    setItems((previous) => new Map(previous).set(id, record));
    return () => {
      setItems((previous) => {
        const next = new Map(previous);
        next.delete(id);
        return next;
      });
    };
  }, []);

  const { visible, visibleGroups } = React.useMemo(() => {
    const ids = new Set<string>();
    const groups = new Set<string>();
    for (const [id, record] of items) {
      if (filter(record.value, search, record.keywords) > 0) {
        ids.add(id);
        if (record.groupId) {
          groups.add(record.groupId);
        }
      }
    }
    return { visible: ids, visibleGroups: groups };
  }, [items, search, filter]);

  // Keep the active option on a visible, enabled item: the first one when it disappears.
  React.useEffect(() => {
    const active = activeId ? items.get(activeId) : undefined;
    if (activeId && active && visible.has(activeId) && !active.disabled) {
      return;
    }
    const first = enabledOptions(rootRef.current)[0]?.id;
    setActiveId(first);
  }, [activeId, items, visible]);

  const select = React.useCallback((id: string) => {
    selectHandlers.current.get(id)?.();
  }, []);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    onKeyDown?.(event);
    if (event.defaultPrevented || event.nativeEvent.isComposing) {
      return;
    }
    const options = enabledOptions(rootRef.current);
    const index = options.findIndex((option) => option.id === activeId);
    let next: HTMLElement | undefined;
    switch (event.key) {
      case "ArrowDown":
        next = index === -1 ? options[0] : (options[index + 1] ?? (loop ? options[0] : undefined));
        break;
      case "ArrowUp":
        next = index <= 0 ? (loop ? options[options.length - 1] : options[0]) : options[index - 1];
        break;
      case "Home":
        next = options[0];
        break;
      case "End":
        next = options[options.length - 1];
        break;
      case "Enter":
        if (activeId && visible.has(activeId)) {
          event.preventDefault();
          select(activeId);
        }
        return;
      default:
        return;
    }
    event.preventDefault();
    if (next) {
      setActiveId(next.id);
      next.scrollIntoView?.({ block: "nearest" });
    }
  };

  const context = React.useMemo<CommandContextValue>(
    () => ({
      search,
      setSearch,
      activeId,
      setActiveId,
      listId,
      inputId,
      isHidden: (id) => items.has(id) && !visible.has(id),
      visibleCount: visible.size,
      visibleGroups,
      register,
      select,
      handlers: selectHandlers,
    }),
    [search, setSearch, activeId, listId, inputId, items, visible, visibleGroups, register, select],
  );

  return (
    <CommandContext.Provider value={context}>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: key events bubble from the combobox input; the root only routes them */}
      <div
        ref={rootRef}
        data-slot="command"
        className={cn(
          "flex h-full w-full flex-col overflow-hidden rounded-xl bg-popover text-popover-foreground",
          className,
        )}
        onKeyDown={handleKeyDown}
        {...props}
      />
    </CommandContext.Provider>
  );
}

function enabledOptions(root: HTMLElement | null): HTMLElement[] {
  if (!root) {
    return [];
  }
  return Array.from(
    root.querySelectorAll<HTMLElement>('[role="option"]:not([hidden]):not([aria-disabled="true"])'),
  );
}

function CommandInput({
  className,
  onChange,
  ...props
}: Omit<React.ComponentProps<"input">, "value" | "defaultValue" | "type">) {
  const { search, setSearch, listId, inputId, activeId } = useCommand();
  return (
    <div
      data-slot="command-input-wrapper"
      className="flex items-center gap-2 border-b px-3 has-focus-visible:ring-2 has-focus-visible:ring-ring/50 has-focus-visible:ring-inset"
    >
      <SearchIcon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
      <input
        id={inputId}
        data-slot="command-input"
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={activeId}
        aria-autocomplete="list"
        autoComplete="off"
        spellCheck={false}
        value={search}
        onChange={(event) => {
          setSearch(event.target.value);
          onChange?.(event);
        }}
        className={cn(
          "h-11 w-full min-w-0 bg-transparent text-sm outline-none placeholder:text-muted-foreground disabled:opacity-50",
          className,
        )}
        {...props}
      />
    </div>
  );
}

function CommandList({ className, ...props }: React.ComponentProps<"div">) {
  const { listId, inputId } = useCommand();
  return (
    <div
      id={listId}
      role="listbox"
      aria-labelledby={inputId}
      data-slot="command-list"
      className={cn("max-h-80 scroll-py-1 overflow-x-hidden overflow-y-auto p-1", className)}
      {...props}
    />
  );
}

function CommandEmpty({ className, ...props }: React.ComponentProps<"div">) {
  const { visibleCount } = useCommand();
  if (visibleCount > 0) {
    return null;
  }
  return (
    <div
      role="presentation"
      data-slot="command-empty"
      className={cn("py-6 text-center text-sm text-muted-foreground", className)}
      {...props}
    />
  );
}

export interface CommandGroupProps extends Omit<React.ComponentProps<"div">, "title"> {
  heading?: React.ReactNode;
}

function CommandGroup({ className, heading, children, ...props }: CommandGroupProps) {
  const { visibleGroups } = useCommand();
  const id = React.useId();
  return (
    <GroupContext.Provider value={id}>
      {/* biome-ignore lint/a11y/useSemanticElements: a listbox group cannot be a fieldset */}
      <div
        role="group"
        aria-labelledby={heading ? `${id}-heading` : undefined}
        hidden={!visibleGroups.has(id)}
        data-slot="command-group"
        className={cn("overflow-hidden p-1 text-foreground", className)}
        {...props}
      >
        {heading && (
          <div
            id={`${id}-heading`}
            aria-hidden="true"
            data-slot="command-group-heading"
            className="px-2 py-1.5 text-xs font-medium text-muted-foreground"
          >
            {heading}
          </div>
        )}
        {children}
      </div>
    </GroupContext.Provider>
  );
}

function CommandSeparator({ className, ...props }: React.ComponentProps<"div">) {
  const { search } = useCommand();
  return (
    // biome-ignore lint/a11y/useSemanticElements: an <hr> is not valid inside a listbox
    <div
      role="separator"
      hidden={search.trim().length > 0}
      data-slot="command-separator"
      className={cn("-mx-1 my-1 h-px bg-border", className)}
      {...props}
    />
  );
}

export interface CommandItemProps
  extends Omit<React.ComponentProps<"div">, "onSelect" | "value" | "id"> {
  /** Match text. Defaults to the item's text content, without shortcut hints. */
  value?: string;
  /** Extra words that match the search without being shown. */
  keywords?: string[];
  disabled?: boolean;
  /** Runs on click and on Enter while the item is active. */
  onSelect?: (value: string) => void;
}

/** An item's own label: its text content without shortcut hints. */
function itemText(element: HTMLElement | null): string {
  if (!element) {
    return "";
  }
  const clone = element.cloneNode(true) as HTMLElement;
  for (const shortcut of clone.querySelectorAll('[data-slot="command-shortcut"]')) {
    shortcut.remove();
  }
  return clone.textContent?.trim() ?? "";
}

function CommandItem({
  className,
  value,
  keywords,
  disabled = false,
  onSelect,
  children,
  ...props
}: CommandItemProps) {
  const context = useCommand();
  const groupId = React.useContext(GroupContext);
  const id = React.useId();
  const ref = React.useRef<HTMLDivElement>(null);
  const keywordKey = (keywords ?? []).join("\u0000");
  const [resolved, setResolved] = React.useState(value ?? "");

  const { register, setActiveId } = context;
  React.useLayoutEffect(() => {
    const text = value ?? itemText(ref.current);
    setResolved(text);
    return register(id, {
      value: text,
      keywords: keywordKey ? keywordKey.split("\u0000") : [],
      disabled,
      groupId,
    });
  }, [register, id, value, keywordKey, disabled, groupId]);

  const { handlers } = context;
  const onSelectRef = React.useRef(onSelect);
  onSelectRef.current = onSelect;
  const valueRef = React.useRef(resolved);
  valueRef.current = resolved;
  React.useEffect(() => {
    if (disabled) {
      return;
    }
    handlers.current.set(id, () => onSelectRef.current?.(valueRef.current));
    return () => {
      handlers.current.delete(id);
    };
  }, [handlers, id, disabled]);

  const active = context.activeId === id;

  return (
    // biome-ignore lint/a11y/useFocusableInteractive lint/a11y/useKeyWithClickEvents: options use aria-activedescendant on the input; the input handles the keys
    <div
      ref={ref}
      id={id}
      role="option"
      aria-selected={active}
      aria-disabled={disabled || undefined}
      hidden={context.isHidden(id)}
      data-slot="command-item"
      data-selected={active || undefined}
      data-disabled={disabled || undefined}
      className={cn(
        "relative flex cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none select-none data-disabled:pointer-events-none data-disabled:opacity-50 data-selected:bg-accent data-selected:text-accent-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
        className,
      )}
      onPointerMove={() => {
        if (!disabled && !active) {
          setActiveId(id);
        }
      }}
      onClick={() => {
        if (!disabled) {
          context.select(id);
        }
      }}
      {...props}
    >
      {children}
    </div>
  );
}

function CommandShortcut({ className, ...props }: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="command-shortcut"
      className={cn("ms-auto text-xs tracking-widest text-muted-foreground", className)}
      {...props}
    />
  );
}

export interface CommandDialogProps extends Omit<React.ComponentProps<typeof Dialog>, "children"> {
  children?: React.ReactNode;
  /** Accessible name of the dialog; visually hidden. */
  title?: string;
  /** Accessible description of the dialog; visually hidden. */
  description?: string;
  /** Props for the inner `Command`. */
  commandProps?: CommandProps;
  container?: React.ComponentProps<typeof DialogContent>["container"];
}

/** A `Command` inside a modal dialog, opened by the app from its ⌘K binding. */
function CommandDialog({
  title = "Command palette",
  description = "Search for a command and press Enter to run it.",
  children,
  commandProps,
  container,
  ...props
}: CommandDialogProps) {
  return (
    <Dialog {...props}>
      <DialogContent
        showClose={false}
        container={container}
        className="top-[15dvh] m-0 mx-auto h-fit gap-0 overflow-hidden p-0"
      >
        <DialogHeader className="sr-only">
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <Command {...commandProps}>{children}</Command>
      </DialogContent>
    </Dialog>
  );
}

export {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
};
