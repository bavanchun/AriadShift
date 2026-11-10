"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ToolDescriptor } from "../../converter/tool-catalog";

type Schema = Record<string, unknown>;
export interface ParamsValidation {
  isValid: boolean;
  errors: Record<string, string>;
}
export interface ParamsFormProps {
  tool: Pick<ToolDescriptor, "id" | "params_schema" | "secret_params">;
  initialParams?: Record<string, unknown>;
  pageCount?: number;
  inputs?: Array<{ id: string; name: string }>;
  disabled?: boolean;
  /** Surface-owned translations, keyed by parameter name. */
  labels?: Record<string, string>;
  translateError?: (error: string) => string;
  onChange: (params: Record<string, unknown>, validation: ParamsValidation) => void;
}

const object = (value: unknown): Schema =>
  typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Schema) : {};

function resolve(schema: Schema, root: Schema, depth = 0): Schema {
  if (typeof schema.$ref !== "string" || !schema.$ref.startsWith("#/") || depth > 12) return schema;
  let target: unknown = root;
  for (const part of schema.$ref.slice(2).split("/")) {
    target = object(target)[part.replaceAll("~1", "/").replaceAll("~0", "~")];
  }
  const { $ref: _ref, ...rest } = schema;
  return { ...resolve(object(target), root, depth + 1), ...rest };
}

function choices(schema: Schema, root: Schema, depth = 0): unknown[] {
  if (depth > 12) return [];
  const resolved = resolve(schema, root);
  if (Array.isArray(resolved.enum)) return resolved.enum;
  if ("const" in resolved) return [resolved.const];
  const variants = resolved.oneOf ?? resolved.anyOf;
  return Array.isArray(variants)
    ? variants.flatMap((variant) => choices(object(variant), root, depth + 1))
    : [];
}

/** Mirrors the shared PageRange grammar without expanding a potentially huge selection. */
export function validatePageRange(value: string, pageCount?: number): string | null {
  if (!value.trim()) return "Enter a page range.";
  if (pageCount !== undefined && pageCount < 1) return "The document has no pages.";
  const ref = "(?:[1-9]\\d*|last(?:-[1-9]\\d*)?)";
  const range = new RegExp(`^(${ref})?-(${ref})?$`);
  for (const token of value.split(",").map((part) => part.trim())) {
    if (["all", "even", "odd"].includes(token)) {
      if (token === "even" && pageCount === 1) return "The page selection is empty.";
      continue;
    }
    const single = new RegExp(`^${ref}$`).test(token);
    const match = single ? null : token.match(range);
    if (!single && (!match || (!match[1] && !match[2]))) return `Invalid page range: ${token}`;
    for (const endpoint of single ? [token] : [match?.[1], match?.[2]]) {
      if (!endpoint) continue;
      if (endpoint.startsWith("last")) {
        const offset = endpoint === "last" ? 0 : Number(endpoint.slice(5));
        if (
          !Number.isSafeInteger(offset) ||
          offset > 0xffffffff ||
          (pageCount !== undefined && offset >= pageCount)
        )
          return `Page exceeds page count: ${endpoint}`;
      } else {
        const number = Number(endpoint);
        if (
          !Number.isSafeInteger(number) ||
          number > 0xffffffff ||
          (pageCount !== undefined && number > pageCount)
        )
          return `Page exceeds page count: ${endpoint}`;
      }
    }
  }
  return null;
}

function enumOnly(original: Schema, root: Schema, depth = 0): boolean {
  if (depth > 12) return false;
  const schema = resolve(original, root);
  if (Array.isArray(schema.enum) || "const" in schema) return true;
  const variants = schema.oneOf ?? schema.anyOf;
  return (
    Array.isArray(variants) &&
    variants.every((variant) => enumOnly(object(variant), root, depth + 1))
  );
}

function pageRange(name: string, schema: Schema): boolean {
  return (
    /(?:^|\/)PageRange$/.test(String(schema.$ref ?? "")) ||
    name === "pages" ||
    schema.format === "page-range"
  );
}

function validateValue(
  value: unknown,
  original: Schema,
  root: Schema,
  name: string,
  pageCount?: number,
  depth = 0,
): string | null {
  if (depth > 24) return "Parameter nesting is too deep.";
  const schema = resolve(original, root);
  const variants = schema.anyOf ?? schema.oneOf;
  if (
    Array.isArray(variants) &&
    !variants.some(
      (variant) => !validateValue(value, object(variant), root, name, pageCount, depth + 1),
    )
  ) {
    return "Enter a value matching an allowed schema.";
  }
  const types = Array.isArray(schema.type) ? schema.type : schema.type ? [schema.type] : [];
  const type = value === null ? "null" : Array.isArray(value) ? "array" : typeof value;
  if (
    types.length &&
    !types.includes(type) &&
    !(types.includes("integer") && typeof value === "number" && Number.isInteger(value))
  )
    return `Expected ${types.join(" or ")}.`;
  if (Array.isArray(schema.enum) && !schema.enum.some((option) => Object.is(option, value)))
    return "Choose an allowed value.";
  if ("const" in schema && !Object.is(schema.const, value)) return "Choose an allowed value.";
  if (typeof value === "number") {
    if (!Number.isFinite(value)) return "Enter a finite number.";
    if (typeof schema.minimum === "number" && value < schema.minimum)
      return `Minimum: ${schema.minimum}`;
    if (typeof schema.maximum === "number" && value > schema.maximum)
      return `Maximum: ${schema.maximum}`;
    if (typeof schema.exclusiveMinimum === "number" && value <= schema.exclusiveMinimum)
      return `Must exceed ${schema.exclusiveMinimum}.`;
    if (typeof schema.exclusiveMaximum === "number" && value >= schema.exclusiveMaximum)
      return `Must be below ${schema.exclusiveMaximum}.`;
  }
  if (typeof value === "string") {
    if (typeof schema.minLength === "number" && value.length < schema.minLength)
      return `Minimum length: ${schema.minLength}`;
    if (typeof schema.maxLength === "number" && value.length > schema.maxLength)
      return `Maximum length: ${schema.maxLength}`;
    if (typeof schema.pattern === "string" && !new RegExp(schema.pattern).test(value))
      return "Enter a value in the required format.";
    if (pageRange(name, original)) return validatePageRange(value, pageCount);
  }
  if (Array.isArray(value)) {
    if (typeof schema.minItems === "number" && value.length < schema.minItems)
      return `Minimum items: ${schema.minItems}`;
    if (typeof schema.maxItems === "number" && value.length > schema.maxItems)
      return `Maximum items: ${schema.maxItems}`;
    if (
      schema.uniqueItems === true &&
      new Set(value.map((item) => JSON.stringify(item))).size !== value.length
    )
      return "Items must be unique.";
    const items = Array.isArray(schema.prefixItems) ? schema.prefixItems : [];
    for (const [index, item] of value.entries()) {
      const error = validateValue(
        item,
        object(items[index] ?? schema.items),
        root,
        `${name}[${index}]`,
        pageCount,
        depth + 1,
      );
      if (error) return error;
    }
  }
  if (type === "object") {
    const properties = object(schema.properties);
    for (const key of Array.isArray(schema.required) ? schema.required : []) {
      if (!(String(key) in object(value))) return `Required: ${key}`;
    }
    for (const [key, item] of Object.entries(object(value))) {
      if (!(key in properties) && schema.additionalProperties === false)
        return `Unknown parameter: ${key}`;
      const error = validateValue(
        item,
        object(properties[key] ?? schema.additionalProperties),
        root,
        key,
        pageCount,
        depth + 1,
      );
      if (error) return `${key}: ${error}`;
    }
  }
  return null;
}

export function validateParams(
  schema: Schema,
  params: Record<string, unknown>,
  pageCount?: number,
): ParamsValidation {
  const errors: Record<string, string> = {};
  const properties = object(schema.properties);
  for (const key of Array.isArray(schema.required) ? schema.required : []) {
    if (!(String(key) in params) || params[String(key)] === "")
      errors[String(key)] = "This parameter is required.";
  }
  for (const [key, value] of Object.entries(params)) {
    if (!(key in properties) && schema.additionalProperties === false)
      errors[key] = "Unknown parameter.";
    else {
      const error = validateValue(value, object(properties[key]), schema, key, pageCount);
      if (error) errors[key] = error;
    }
  }
  return { isValid: Object.keys(errors).length === 0, errors };
}

/** A tool change remounts the private draft, dropping any passwords from the previous tool. */
export function ParamsForm(props: ParamsFormProps) {
  return <ParamsDraft key={props.tool.id} {...props} />;
}

function ParamsDraft({
  tool,
  initialParams = {},
  pageCount,
  inputs = [],
  disabled,
  labels = {},
  translateError = (error) => error,
  onChange,
}: ParamsFormProps) {
  const prefix = useId();
  const properties = useMemo(() => object(tool.params_schema.properties), [tool.params_schema]);
  const [params, setParams] = useState<Record<string, unknown>>(() => ({
    ...Object.fromEntries(
      Object.entries(properties).flatMap(([key, raw]) => {
        const schema = resolve(object(raw), tool.params_schema);
        return "default" in schema &&
          !tool.secret_params.includes(key) &&
          schema["x-ariad-secret"] !== true &&
          schema.format !== "password"
          ? [[key, schema.default]]
          : [];
      }),
    ),
    ...initialParams,
  }));
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [parseErrors, setParseErrors] = useState<Record<string, string>>({});
  const validation = useMemo(() => {
    const checked = validateParams(tool.params_schema, params, pageCount);
    const errors = { ...checked.errors, ...parseErrors };
    return { isValid: Object.keys(errors).length === 0, errors };
  }, [tool.params_schema, params, pageCount, parseErrors]);
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);
  const lastEmission = useRef<string | null>(null);
  useEffect(() => {
    const emission = JSON.stringify({ params, validation });
    if (lastEmission.current === emission) return;
    lastEmission.current = emission;
    onChangeRef.current(params, validation);
  }, [params, validation]);

  function update(key: string, value: unknown, error?: string) {
    setParams((previous) => {
      const next = { ...previous };
      if (value === undefined) delete next[key];
      else next[key] = value;
      return next;
    });
    setParseErrors((previous) => {
      const next = { ...previous };
      if (error) next[key] = error;
      else delete next[key];
      return next;
    });
  }

  return (
    <div className="grid gap-4">
      {Object.entries(properties).map(([key, raw]) => {
        const original = object(raw);
        const schema = resolve(original, tool.params_schema);
        const id = `${prefix}-${key}`;
        const error = validation.errors[key];
        const secret =
          tool.secret_params.includes(key) ||
          schema["x-ariad-secret"] === true ||
          schema.format === "password";
        const types = Array.isArray(schema.type) ? schema.type : [schema.type];
        const type = types.find((item) => item !== "null");
        const options = choices(original, tool.params_schema);
        const closedEnum = enumOnly(original, tool.params_schema);
        const isPaper =
          /(?:^|\/)PaperSize$/.test(String(original.$ref ?? "")) || schema.format === "paper-size";
        const file =
          ["file", "binary"].includes(String(schema.format)) || schema["x-ariad-input"] === true;
        const numeric = type === "number" || type === "integer";
        const json =
          type === "object" ||
          (type === "array" && object(schema.items).type !== "string") ||
          (!type && !closedEnum && !isPaper);
        const value = params[key];
        const common = {
          id,
          disabled,
          "aria-invalid": !!error,
          "aria-describedby": error ? `${id}-error` : undefined,
          className: "w-full rounded border bg-transparent p-2",
        };
        const enumOptions = isPaper
          ? [...new Set(["A4", "Letter", "A3", "Legal", "A5", ...options])]
          : options;
        return (
          <div key={key} className="grid gap-1">
            <label htmlFor={id}>
              {labels[key] ?? String(schema.title ?? key.replaceAll("_", " "))}
            </label>
            {type === "boolean" && !secret ? (
              <input
                {...common}
                type="checkbox"
                checked={value === true}
                onChange={(event) => update(key, event.target.checked)}
              />
            ) : !secret && (closedEnum || isPaper || file) ? (
              <select
                {...common}
                value={value === undefined || value === null ? "" : String(value)}
                onChange={(event) => {
                  const selected = enumOptions.find(
                    (option) => String(option) === event.target.value,
                  );
                  update(
                    key,
                    event.target.value === "" ? undefined : file ? event.target.value : selected,
                  );
                }}
              >
                <option value="" />
                {(file
                  ? inputs.map((input) => ({ value: input.id, label: input.name }))
                  : enumOptions.map((option) => ({ value: String(option), label: String(option) }))
                ).map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            ) : json && !secret ? (
              <textarea
                {...common}
                value={drafts[key] ?? (value === undefined ? "" : JSON.stringify(value, null, 2))}
                onChange={(event) => {
                  const text = event.target.value;
                  setDrafts((previous) => ({ ...previous, [key]: text }));
                  if (!text.trim()) update(key, undefined);
                  else
                    try {
                      update(key, JSON.parse(text));
                    } catch {
                      update(key, undefined, "Enter valid JSON.");
                    }
                }}
              />
            ) : (
              <input
                {...common}
                type={
                  secret
                    ? "password"
                    : numeric
                      ? "number"
                      : /(?:color|colour)/i.test(key) || schema.pattern === "^#[0-9A-Fa-f]{6}$"
                        ? "color"
                        : "text"
                }
                autoComplete={secret ? "new-password" : "off"}
                spellCheck={false}
                min={typeof schema.minimum === "number" ? schema.minimum : undefined}
                max={typeof schema.maximum === "number" ? schema.maximum : undefined}
                step={type === "integer" ? 1 : "any"}
                value={
                  drafts[key] ??
                  (Array.isArray(value) ? value.join(", ") : value == null ? "" : String(value))
                }
                onChange={(event) => {
                  const text = event.target.value;
                  setDrafts((previous) => ({ ...previous, [key]: text }));
                  update(
                    key,
                    text === ""
                      ? undefined
                      : numeric
                        ? Number(text)
                        : type === "array"
                          ? text
                              .split(",")
                              .map((tag) => tag.trim())
                              .filter(Boolean)
                          : text,
                  );
                }}
              />
            )}
            {error ? (
              <p id={`${id}-error`} role="alert">
                {translateError(error)}
              </p>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
