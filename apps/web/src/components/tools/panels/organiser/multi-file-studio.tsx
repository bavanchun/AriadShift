"use client";
// `pdf.merge` and `pdf.alternate_mix`: the file order and, for the mix, the interleave diagram.
import { Badge } from "@ariadshift/ui/components/badge";
import { useMemo, useState } from "react";
import type { WorkspaceFile } from "@/components/workspace/types";
import type { CustomToolPanelProps } from "../types";
import { organiserCopy } from "./copy";
import { controlClass, ErrorText, Field, PagePill, useEmitParams } from "./panel-kit";

type MultiProps = CustomToolPanelProps;
const SAMPLE_PAGES = 4;
const DIAGRAM_SLOTS = 14;

/** Input-count errors derived from the tool's declared arity. */
function arityError(tool: MultiProps["tool"], count: number): string | undefined {
  if (tool.inputs.arity !== "many") return undefined;
  if (count < tool.inputs.min) return organiserCopy.merge.needsMore(tool.inputs.min);
  if (count > tool.inputs.max) return organiserCopy.merge.tooMany(tool.inputs.max);
  return undefined;
}

function FileOrder({ files }: { files: WorkspaceFile[] }) {
  const copy = organiserCopy.merge;
  const known = files.every((file) => file.pageCount !== null);
  const total = known ? files.reduce((sum, file) => sum + (file.pageCount ?? 0), 0) : null;
  return (
    <div className="grid gap-2" data-slot="merge-order">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium text-start">{copy.order}</span>
        <Badge variant="secondary" data-slot="merge-total">
          {copy.total(files.length, total)}
        </Badge>
      </div>
      <ol aria-label={copy.order} className="grid gap-1.5">
        {files.map((file, index) => (
          <li
            key={file.key}
            className="flex items-center gap-2 rounded-lg border border-border bg-background px-2.5 py-1.5 text-sm"
          >
            <PagePill label={index + 1} />
            <span className="min-w-0 flex-1 truncate text-start">{file.name}</span>
            <span className="text-xs text-muted-foreground">{copy.pages(file.pageCount)}</span>
          </li>
        ))}
      </ol>
      <p className="text-xs text-muted-foreground text-start">{copy.orderHint}</p>
    </div>
  );
}

function ModeSelect<T extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  disabled?: boolean;
  onChange: (value: T) => void;
}) {
  return (
    <Field label={label}>
      {({ id }) => (
        <select
          id={id}
          disabled={disabled}
          className={`${controlClass} w-full text-start`}
          value={value}
          onChange={(event) => onChange(event.target.value as T)}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      )}
    </Field>
  );
}

export function MergeStudio({ tool, files, disabled, onChange }: MultiProps) {
  const copy = organiserCopy.merge;
  const [outline, setOutline] = useState<"keep" | "per-file" | "none">("per-file");
  const [pageLabels, setPageLabels] = useState<"keep" | "renumber">("renumber");
  const params = useMemo(() => ({ outline, page_labels: pageLabels }), [outline, pageLabels]);
  const problem = arityError(tool, files.length);
  const extraErrors = useMemo(() => (problem ? { inputs: problem } : {}), [problem]);
  const validation = useEmitParams(tool, params, { extraErrors, onChange });
  return (
    <div className="grid gap-4">
      <FileOrder files={files} />
      <ErrorText>{validation.errors.inputs}</ErrorText>
      <div className="grid gap-4 sm:grid-cols-2">
        <ModeSelect
          label={copy.outline}
          value={outline}
          disabled={disabled}
          onChange={setOutline}
          options={[
            { value: "per-file", label: copy.perFile },
            { value: "keep", label: copy.keep },
            { value: "none", label: copy.none },
          ]}
        />
        <ModeSelect
          label={copy.pageLabels}
          value={pageLabels}
          disabled={disabled}
          onChange={setPageLabels}
          options={[
            { value: "renumber", label: copy.renumber },
            { value: "keep", label: copy.keep },
          ]}
        />
      </div>
    </div>
  );
}

/** The page sequence `pdf.alternate_mix` produces: A1, B1, A2, B2 … with leftovers appended. */
export function interleave(
  countA: number,
  countB: number,
  reverseSecond: boolean,
): Array<{ from: "a" | "b"; page: number }> {
  const slots: Array<{ from: "a" | "b"; page: number }> = [];
  for (let index = 0; index < Math.max(countA, countB); index++) {
    if (index < countA) slots.push({ from: "a", page: index + 1 });
    if (index < countB) slots.push({ from: "b", page: reverseSecond ? countB - index : index + 1 });
  }
  return slots;
}

export function AlternateMixStudio({ tool, files, disabled, onChange }: MultiProps) {
  const copy = organiserCopy.mix;
  const [reverse, setReverse] = useState(false);
  const params = useMemo(() => ({ reverse_second: reverse }), [reverse]);
  const problem = arityError(tool, files.length);
  const extraErrors = useMemo(() => (problem ? { inputs: problem } : {}), [problem]);
  const validation = useEmitParams(tool, params, { extraErrors, onChange });
  const [first, second] = files;
  const sample = first?.pageCount == null || second?.pageCount == null;
  const slots = useMemo(
    () => interleave(first?.pageCount ?? SAMPLE_PAGES, second?.pageCount ?? SAMPLE_PAGES, reverse),
    [first?.pageCount, second?.pageCount, reverse],
  );
  const id = `mix-reverse-${tool.id}`;
  return (
    <div className="grid gap-4">
      <div className="grid gap-2 sm:grid-cols-2">
        {[first, second].map((file, index) => (
          <div
            key={index === 0 ? "a" : "b"}
            className="flex items-center gap-2 rounded-lg border border-border bg-background px-2.5 py-1.5 text-sm"
          >
            <PagePill tone={index === 0 ? "a" : "b"} label={index === 0 ? "A" : "B"} />
            <span className="min-w-0 flex-1 truncate text-start">
              {file?.name ?? (index === 0 ? copy.fileA : copy.fileB)}
            </span>
          </div>
        ))}
      </div>
      <ErrorText>{validation.errors.inputs}</ErrorText>
      <label htmlFor={id} className="flex items-center gap-2 text-sm text-start">
        <input
          id={id}
          type="checkbox"
          disabled={disabled}
          className="size-4 cursor-pointer rounded-sm border border-input accent-primary"
          checked={reverse}
          onChange={(event) => setReverse(event.target.checked)}
        />
        {copy.reverse}
      </label>
      <div className="grid gap-2" data-slot="mix-diagram">
        <span className="text-sm font-medium text-start">{copy.diagram}</span>
        <ol aria-label={copy.diagram} className="flex flex-wrap gap-1">
          {slots.slice(0, DIAGRAM_SLOTS).map((slot) => (
            <li key={`${slot.from}${slot.page}`}>
              <PagePill
                tone={slot.from}
                source={slot.from}
                label={`${slot.from.toUpperCase()}${slot.page}`}
              />
            </li>
          ))}
          {slots.length > DIAGRAM_SLOTS ? <li aria-hidden="true">{copy.more}</li> : null}
        </ol>
        {sample ? <p className="text-xs text-muted-foreground text-start">{copy.example}</p> : null}
      </div>
    </div>
  );
}
