"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { cn } from "@ariadshift/ui/lib/utils";
import { CheckIcon, EyeIcon, EyeOffIcon, MinusIcon } from "lucide-react";
import { useId, useMemo, useState } from "react";
import {
  CONTROL_CLASS,
  type CustomToolPanelProps,
  useEmitParams,
} from "../compress/panel-contract";
import {
  ALL_PERMITTED,
  buildPermissions,
  countSanitized,
  FORM_MODES,
  METADATA_FIELDS,
  METADATA_LABELS,
  PERMISSION_ROWS,
  type PermissionKey,
  type PermissionState,
  passwordStrength,
  SANITIZE_DEFAULTS,
  SANITIZE_ROWS,
  type SanitizeForms,
  type SanitizeState,
  validation,
} from "./security-model";

const STRENGTH_LABELS = {
  empty: "No password",
  weak: "Weak",
  fair: "Fair",
  good: "Good",
  strong: "Strong",
} as const;

/** A tool change remounts the draft, so a password never carries over to another tool. */
export function SecurityPanel(props: CustomToolPanelProps) {
  return <SecurityDraft key={props.tool.id} {...props} />;
}

function SecurityDraft({ tool, disabled, onChange }: CustomToolPanelProps) {
  const id = tool.id;
  const needsOwner = id === "pdf.encrypt" || id === "pdf.permissions";
  const hasPermissions = needsOwner;

  // Secrets live only in this component's state: never storage, never the URL.
  const [ownerPassword, setOwnerPassword] = useState("");
  const [userPassword, setUserPassword] = useState("");
  const [openPassword, setOpenPassword] = useState("");
  const [algorithm, setAlgorithm] = useState<"aes-256" | "aes-128">("aes-256");
  const [permissions, setPermissions] = useState<PermissionState>(ALL_PERMITTED);
  const [sanitize, setSanitize] = useState<SanitizeState>(SANITIZE_DEFAULTS);
  const [stripAll, setStripAll] = useState(true);
  const [removeFields, setRemoveFields] = useState<string[]>([]);

  const { params, checked } = useMemo(() => {
    const out: Record<string, unknown> = {};
    const errors: Record<string, string> = {};
    if (id === "pdf.encrypt" || id === "pdf.permissions") {
      if (id === "pdf.encrypt") out.algorithm = algorithm;
      out.owner_password = ownerPassword;
      if (userPassword) out.user_password = userPassword;
      out.permissions = buildPermissions(permissions);
      if (!ownerPassword) errors.owner_password = "Enter an owner password.";
    } else if (id === "pdf.decrypt") {
      if (openPassword) out.password = openPassword;
    } else if (id === "pdf.sanitize") {
      Object.assign(out, sanitize);
    } else if (id === "pdf.metadata") {
      out.strip_all = stripAll;
      if (!stripAll) {
        out.remove = removeFields;
        if (removeFields.length === 0) errors.remove = "Choose at least one field to remove.";
      }
    }
    return { params: out, checked: validation(errors) };
  }, [
    id,
    algorithm,
    ownerPassword,
    userPassword,
    openPassword,
    permissions,
    sanitize,
    stripAll,
    removeFields,
  ]);
  useEmitParams(params, checked, onChange);

  return (
    <div data-slot="security-panel" className="grid gap-5">
      {needsOwner ? (
        <section aria-label="Passwords" className="grid gap-4">
          {id === "pdf.encrypt" ? (
            <div className="flex flex-wrap items-center gap-3">
              <Badge variant="secondary" data-testid="cipher-badge">
                {algorithm === "aes-256" ? "AES-256" : "AES-128"}
              </Badge>
              <label className="flex items-center gap-2 text-sm">
                <span className="font-medium">Cipher</span>
                <select
                  className={cn(CONTROL_CLASS, "h-9")}
                  disabled={disabled}
                  value={algorithm}
                  onChange={(event) => setAlgorithm(event.target.value as "aes-256" | "aes-128")}
                >
                  <option value="aes-256">AES-256 (recommended)</option>
                  <option value="aes-128">AES-128 (older readers)</option>
                </select>
              </label>
            </div>
          ) : null}
          <PasswordField
            name="owner"
            label="Owner password"
            hint="Needed to change the permissions or remove the protection."
            value={ownerPassword}
            onValue={setOwnerPassword}
            required
            error={checked.errors.owner_password}
            meter
            disabled={disabled}
          />
          <PasswordField
            name="user"
            label="User password (optional)"
            hint="Needed to open the document. Leave empty to let anyone open it."
            value={userPassword}
            onValue={setUserPassword}
            meter
            disabled={disabled}
          />
        </section>
      ) : null}

      {id === "pdf.decrypt" ? (
        <section aria-label="Password" className="grid gap-4">
          <PasswordField
            name="open"
            label="Document password"
            hint="Leave empty if the document opens without a password."
            value={openPassword}
            onValue={setOpenPassword}
            disabled={disabled}
          />
        </section>
      ) : null}

      {hasPermissions ? (
        <PermissionsMatrix state={permissions} onChange={setPermissions} disabled={disabled} />
      ) : null}

      {id === "pdf.sanitize" ? (
        <SanitizeChecklist state={sanitize} onChange={setSanitize} disabled={disabled} />
      ) : null}

      {id === "pdf.metadata" ? (
        <MetadataChecklist
          stripAll={stripAll}
          onStripAll={setStripAll}
          fields={removeFields}
          onFields={setRemoveFields}
          error={checked.errors.remove}
          disabled={disabled}
        />
      ) : null}

      {id === "pdf.unlock_forms" ? (
        <p className="text-sm text-muted-foreground">
          Makes the form fields editable again by clearing their read-only flags. It does not remove
          a password or the document's permissions.
        </p>
      ) : null}

      {id === "pdf.encrypt" || id === "pdf.permissions" || id === "pdf.decrypt" ? (
        <p className="text-xs text-muted-foreground">
          Passwords stay in this tab's memory while you work. They are never saved or put in the
          address bar.
        </p>
      ) : null}
    </div>
  );
}

interface PasswordFieldProps {
  name: string;
  label: string;
  hint?: string;
  value: string;
  onValue: (value: string) => void;
  required?: boolean;
  error?: string;
  meter?: boolean;
  disabled?: boolean;
}

function PasswordField({
  name,
  label,
  hint,
  value,
  onValue,
  required,
  error,
  meter,
  disabled,
}: PasswordFieldProps) {
  const id = `${useId()}-${name}`;
  const [shown, setShown] = useState(false);
  const [touched, setTouched] = useState(false);
  const strength = passwordStrength(value);
  const showError = !!error && touched;
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <div className="flex gap-2">
        <input
          id={id}
          type={shown ? "text" : "password"}
          autoComplete="new-password"
          autoCapitalize="off"
          spellCheck={false}
          disabled={disabled}
          required={required}
          aria-invalid={showError}
          aria-describedby={showError ? `${id}-error` : hint ? `${id}-hint` : undefined}
          value={value}
          onChange={(event) => onValue(event.target.value)}
          onBlur={() => setTouched(true)}
          className={cn(CONTROL_CLASS, "h-9 w-full")}
        />
        <button
          type="button"
          aria-label={shown ? `Hide ${label}` : `Show ${label}`}
          aria-pressed={shown}
          disabled={disabled}
          onClick={() => setShown((current) => !current)}
          className="inline-flex size-9 shrink-0 cursor-pointer items-center justify-center rounded-lg border border-input bg-background outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {shown ? <EyeOffIcon className="size-4" /> : <EyeIcon className="size-4" />}
        </button>
      </div>
      {showError ? (
        <p id={`${id}-error`} role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : hint ? (
        <p id={`${id}-hint`} className="text-xs text-muted-foreground">
          {hint}
        </p>
      ) : null}
      {meter ? (
        <div className="flex items-center gap-2">
          <meter
            aria-label={`${label} strength`}
            min={0}
            max={4}
            low={2}
            high={3}
            optimum={4}
            value={strength.score}
            className="h-2 w-full"
          />
          <span data-testid={`strength-${name}`} className="shrink-0 text-xs text-muted-foreground">
            {STRENGTH_LABELS[strength.level]}
            {strength.bits > 0 ? ` · ${strength.bits} bits` : ""}
          </span>
        </div>
      ) : null}
    </div>
  );
}

function PermissionsMatrix({
  state,
  onChange,
  disabled,
}: {
  state: PermissionState;
  onChange: (state: PermissionState) => void;
  disabled?: boolean;
}) {
  return (
    <fieldset className="grid gap-2" disabled={disabled} data-slot="permissions-matrix">
      <legend className="mb-1 text-sm font-medium">Permissions</legend>
      <p className="text-xs text-muted-foreground">
        Switch off what readers who open the document with the user password may not do.
      </p>
      {PERMISSION_ROWS.map((row) => (
        <label
          key={row.key}
          className="flex cursor-pointer items-start gap-3 rounded-lg border border-input px-3 py-2"
        >
          <input
            type="checkbox"
            className="mt-0.5 size-4 cursor-pointer accent-primary"
            checked={state[row.key]}
            onChange={(event) =>
              onChange({ ...state, [row.key as PermissionKey]: event.target.checked })
            }
          />
          <span className="grid">
            <span className="text-sm font-medium">{row.label}</span>
            <span className="text-xs text-muted-foreground">{row.detail}</span>
          </span>
        </label>
      ))}
    </fieldset>
  );
}

function ChecklistRow({
  label,
  detail,
  on,
  onToggle,
  disabled,
}: {
  label: string;
  detail: string;
  on: boolean;
  onToggle: (next: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-3 rounded-lg border border-input px-3 py-2">
      <input
        type="checkbox"
        className="size-4 cursor-pointer accent-primary"
        checked={on}
        disabled={disabled}
        onChange={(event) => onToggle(event.target.checked)}
      />
      <span className="grid flex-1">
        <span className="text-sm font-medium">{label}</span>
        <span className="text-xs text-muted-foreground">{detail}</span>
      </span>
      <span
        className={cn(
          "inline-flex items-center gap-1 text-xs",
          on ? "text-destructive" : "text-muted-foreground",
        )}
      >
        {on ? <CheckIcon className="size-3.5" /> : <MinusIcon className="size-3.5" />}
        {on ? "Will be removed" : "Kept"}
      </span>
    </label>
  );
}

function SanitizeChecklist({
  state,
  onChange,
  disabled,
}: {
  state: SanitizeState;
  onChange: (state: SanitizeState) => void;
  disabled?: boolean;
}) {
  const removed = countSanitized(state);
  return (
    <fieldset className="grid gap-2" disabled={disabled} data-slot="sanitize-checklist">
      <legend className="mb-1 text-sm font-medium">Security audit</legend>
      <p className="text-xs text-muted-foreground" data-testid="sanitize-summary">
        {removed === 0
          ? "Nothing will be removed."
          : `${removed} kinds of hidden content will be removed.`}
      </p>
      {SANITIZE_ROWS.map((row) => (
        <ChecklistRow
          key={row.key}
          label={row.label}
          detail={row.detail}
          on={state[row.key]}
          onToggle={(next) => onChange({ ...state, [row.key]: next })}
          disabled={disabled}
        />
      ))}
      <label className="grid gap-1.5 text-sm font-medium">
        Form submit actions
        <select
          className={cn(CONTROL_CLASS, "h-9 w-full font-normal")}
          value={state.forms}
          onChange={(event) => onChange({ ...state, forms: event.target.value as SanitizeForms })}
        >
          {FORM_MODES.map((mode) => (
            <option key={mode.value} value={mode.value}>
              {mode.label}
            </option>
          ))}
        </select>
      </label>
    </fieldset>
  );
}

function MetadataChecklist({
  stripAll,
  onStripAll,
  fields,
  onFields,
  error,
  disabled,
}: {
  stripAll: boolean;
  onStripAll: (value: boolean) => void;
  fields: string[];
  onFields: (fields: string[]) => void;
  error?: string;
  disabled?: boolean;
}) {
  return (
    <fieldset className="grid gap-2" disabled={disabled} data-slot="metadata-checklist">
      <legend className="mb-1 text-sm font-medium">Metadata to strip</legend>
      <ChecklistRow
        label="Strip all metadata"
        detail="Clears the info dictionary and the XMP stream"
        on={stripAll}
        onToggle={onStripAll}
        disabled={disabled}
      />
      {stripAll
        ? null
        : METADATA_FIELDS.map((field) => (
            <ChecklistRow
              key={field}
              label={METADATA_LABELS[field]}
              detail={`/${field}`}
              on={fields.includes(field)}
              onToggle={(next) =>
                onFields(next ? [...fields, field] : fields.filter((item) => item !== field))
              }
              disabled={disabled}
            />
          ))}
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : null}
    </fieldset>
  );
}
