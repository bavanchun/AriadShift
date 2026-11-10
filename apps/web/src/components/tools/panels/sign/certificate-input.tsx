"use client";

import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { ShieldCheckIcon } from "lucide-react";
import { CheckRow, controlClass, Field, sectionTitleClass } from "../ocr/panel-kit";
import { signCopy } from "./copy";

/** What the panel holds about the certificate. It lives in component state only. */
export interface CertificateDraft {
  file: File | null;
  passphrase: string;
  noPassphrase: boolean;
}

export const EMPTY_CERTIFICATE: CertificateDraft = {
  file: null,
  passphrase: "",
  noPassphrase: false,
};

export const isPkcs12Name = (name: string) => /\.(p12|pfx)$/i.test(name);

/** The first reason the certificate cannot be used yet, keyed by the parameter it belongs to. */
export function certificateErrors(draft: CertificateDraft): Record<string, string> {
  const copy = signCopy.certificate;
  const errors: Record<string, string> = {};
  if (!draft.file) errors.certificate = copy.empty;
  else if (!isPkcs12Name(draft.file.name)) errors.certificate = copy.wrongType;
  if (!draft.noPassphrase && draft.passphrase === "") errors.password = copy.passphraseRequired;
  return errors;
}

/**
 * Picks a PKCS#12 certificate and takes its passphrase. Nothing here is serialised: the file is
 * never read, the passphrase is a controlled input value, and unmounting discards both.
 */
export function CertificateInput({
  draft,
  onChange,
  errors,
  disabled,
}: {
  draft: CertificateDraft;
  onChange: (draft: CertificateDraft) => void;
  errors: Record<string, string>;
  disabled?: boolean;
}) {
  const copy = signCopy.certificate;
  return (
    <fieldset className="grid gap-3 border-0 p-0" disabled={disabled} data-slot="sign-certificate">
      <legend className={sectionTitleClass}>{copy.title}</legend>
      <Field label={copy.file} error={errors.certificate}>
        {({ id, invalid: _invalid, ...aria }) => (
          <input
            {...aria}
            id={id}
            type="file"
            accept=".p12,.pfx,application/x-pkcs12"
            className={`${controlClass} h-auto w-full py-1.5 file:me-2 file:rounded-md file:border-0 file:bg-secondary file:px-2 file:py-1 file:text-sm`}
            onChange={(event) => onChange({ ...draft, file: event.target.files?.[0] ?? null })}
          />
        )}
      </Field>
      {draft.file ? (
        <p className="text-xs text-muted-foreground text-start" data-slot="sign-certificate-name">
          {copy.chosen(draft.file.name, draft.file.size)}
        </p>
      ) : null}
      <Field label={copy.passphrase} error={errors.password}>
        {({ id, invalid: _invalid, ...aria }) => (
          <input
            {...aria}
            id={id}
            type="password"
            autoComplete="off"
            spellCheck={false}
            data-lpignore="true"
            className={`${controlClass} w-full`}
            disabled={draft.noPassphrase}
            value={draft.passphrase}
            onChange={(event) => onChange({ ...draft, passphrase: event.target.value })}
          />
        )}
      </Field>
      <CheckRow
        label={copy.noPassphrase}
        checked={draft.noPassphrase}
        onChange={(checked) =>
          onChange({ ...draft, noPassphrase: checked, passphrase: checked ? "" : draft.passphrase })
        }
      />
      <div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={!draft.file && draft.passphrase === "" && !draft.noPassphrase}
          onClick={() => onChange(EMPTY_CERTIFICATE)}
        >
          {copy.clear}
        </Button>
      </div>
      <Alert data-slot="sign-certificate-note">
        <ShieldCheckIcon aria-hidden="true" />
        <AlertDescription>
          <p>{copy.memoryNote}</p>
          <p>{copy.honestNote}</p>
        </AlertDescription>
      </Alert>
    </fieldset>
  );
}
