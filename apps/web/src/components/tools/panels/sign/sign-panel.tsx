"use client";

import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@ariadshift/ui/components/tabs";
import { TriangleAlertIcon } from "lucide-react";
import { useMemo, useState } from "react";
import {
  CheckRow,
  ChoiceChips,
  type CustomToolPanelProps,
  controlClass,
  ErrorText,
  Field,
  sectionTitleClass,
  useEmitParams,
} from "../ocr/panel-kit";
import {
  type CertificateDraft,
  CertificateInput,
  certificateErrors,
  EMPTY_CERTIFICATE,
} from "./certificate-input";
import { signCopy } from "./copy";
import {
  DEFAULT_PLACEMENT,
  type PlacementDraft,
  PlacementFields,
  parsePlacement,
} from "./placement";
import { type PadPoint, SignaturePad, toVectorData } from "./signature-pad";
import { SignatureValidationCard } from "./validation-card";

export const SIGN_TOOLS = [
  "pdf.sign_visual",
  "pdf.sign",
  "pdf.sign_verify",
  "pdf.sign_remove",
] as const;
export type SignToolId = (typeof SIGN_TOOLS)[number];

export function isSignTool(id: string): id is SignToolId {
  return (SIGN_TOOLS as readonly string[]).includes(id);
}

type Mode = "draw" | "type" | "image";
const MAX_IMAGE_BYTES = 2 * 1024 * 1024;
/** The reference a recipe or CLI run resolves by asking the person at run time. */
const PROMPT = { $secret: "prompt" } as const;

const text = (value: string) => (value.trim() ? value.trim() : undefined);
const optional = (entries: Record<string, string | undefined>) =>
  Object.fromEntries(Object.entries(entries).filter(([, value]) => value !== undefined));

interface SignatureImage {
  name: string;
  dataUrl: string;
  base64: string;
}

function readImage(file: File): Promise<SignatureImage> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(signCopy.image.unreadable));
    reader.onload = () => {
      const dataUrl = String(reader.result);
      const base64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
      if (!base64) reject(new Error(signCopy.image.unreadable));
      else resolve({ name: file.name, dataUrl, base64 });
    };
    reader.readAsDataURL(file);
  });
}

function TypedPreviews({ value }: { value: string }) {
  const copy = signCopy.type;
  const styles = [
    {
      key: "calligraphic",
      label: copy.calligraphic,
      style: {
        fontFamily: '"Snell Roundhand","Brush Script MT","Segoe Script",cursive',
        fontStyle: "italic",
      },
    },
    { key: "serif", label: copy.serif, style: { fontFamily: "Georgia,'Times New Roman',serif" } },
    { key: "sans", label: copy.sans, style: { fontFamily: "system-ui,sans-serif" } },
  ] as const;
  return (
    <ul
      aria-label={copy.previews}
      className="grid gap-2 sm:grid-cols-3"
      data-slot="sign-type-previews"
    >
      {styles.map((style) => (
        <li key={style.key} className="grid gap-1 rounded-lg border border-input bg-background p-3">
          <span className="truncate text-2xl" style={style.style} data-style={style.key}>
            {value || "Ariad"}
          </span>
          <span className="text-xs text-muted-foreground">{style.label}</span>
        </li>
      ))}
    </ul>
  );
}

/** `pdf.sign_visual`: draw, type or upload a signature and place it on a page. */
function VisualStudio({ tool, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const copy = signCopy;
  const [mode, setMode] = useState<Mode>("draw");
  const [points, setPoints] = useState<PadPoint[]>([]);
  const [typed, setTyped] = useState("");
  const [image, setImage] = useState<SignatureImage | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [placementDraft, setPlacementDraft] = useState<PlacementDraft>(DEFAULT_PLACEMENT);
  const [signer, setSigner] = useState("");
  const [color, setColor] = useState("#000000");

  const parsed = parsePlacement(placementDraft, { pageCount });
  const box = parsed.placement
    ? {
        width: parsed.placement.rect[2] - parsed.placement.rect[0],
        height: parsed.placement.rect[3] - parsed.placement.rect[1],
      }
    : { width: 1, height: 1 };

  const params = useMemo<Record<string, unknown>>(() => {
    const body: Record<string, unknown> = {
      mode,
      color,
      ...(parsed.placement ?? { page: Number(placementDraft.page) || 1 }),
      ...optional({ signer_name: text(signer) }),
    };
    if (mode === "draw" && points.length) body.vector_data = toVectorData(points, box);
    if (mode === "type" && typed.trim()) body.text = typed.trim();
    if (mode === "image" && image) body.image_data = image.base64;
    return body;
  }, [mode, color, placementDraft, signer, points, typed, image]);

  const modeError =
    mode === "draw" && points.length === 0
      ? copy.draw.empty
      : mode === "type" && !typed.trim()
        ? copy.type.empty
        : mode === "image" && !image
          ? (imageError ?? copy.image.empty)
          : undefined;
  const validation = useEmitParams(tool, params, {
    extraErrors: {
      ...(parsed.errors.page ? { page: parsed.errors.page } : {}),
      ...(parsed.errors.rect ? { rect: parsed.errors.rect } : {}),
      ...(modeError
        ? { [mode === "draw" ? "vector_data" : mode === "type" ? "text" : "image_data"]: modeError }
        : {}),
    },
    onChange,
  });

  async function chooseImage(file: File | undefined) {
    setImage(null);
    setImageError(null);
    if (!file) return;
    if (!["image/png", "image/jpeg"].includes(file.type))
      return setImageError(copy.image.wrongType);
    if (file.size > MAX_IMAGE_BYTES) return setImageError(copy.image.tooLarge);
    try {
      setImage(await readImage(file));
    } catch (error) {
      setImageError(error instanceof Error ? error.message : copy.image.unreadable);
    }
  }

  return (
    <>
      <section className="grid gap-3" aria-labelledby="sign-appearance-title">
        <h3 id="sign-appearance-title" className={sectionTitleClass}>
          {copy.appearance.title}
        </h3>
        <Tabs value={mode} onValueChange={(value) => setMode(value as Mode)}>
          <TabsList aria-label={copy.appearance.mode}>
            <TabsTrigger value="draw" disabled={disabled}>
              {copy.appearance.draw}
            </TabsTrigger>
            <TabsTrigger value="type" disabled={disabled}>
              {copy.appearance.type}
            </TabsTrigger>
            <TabsTrigger value="image" disabled={disabled}>
              {copy.appearance.image}
            </TabsTrigger>
          </TabsList>
          <TabsContent value="draw" className="grid gap-2">
            <SignaturePad points={points} onChange={setPoints} disabled={disabled} />
          </TabsContent>
          <TabsContent value="type" className="grid gap-3">
            <Field label={copy.type.input}>
              {({ id, invalid: _invalid, ...aria }) => (
                <input
                  {...aria}
                  id={id}
                  type="text"
                  autoComplete="off"
                  className={`${controlClass} w-full`}
                  disabled={disabled}
                  value={typed}
                  onChange={(event) => setTyped(event.target.value)}
                />
              )}
            </Field>
            <TypedPreviews value={typed.trim()} />
            <p className="text-xs text-muted-foreground text-start">{copy.type.note}</p>
          </TabsContent>
          <TabsContent value="image" className="grid gap-2">
            <Field label={copy.image.input} hint={copy.image.hint} error={imageError ?? undefined}>
              {({ id, invalid: _invalid, ...aria }) => (
                <input
                  {...aria}
                  id={id}
                  type="file"
                  accept="image/png,image/jpeg"
                  disabled={disabled}
                  className={`${controlClass} h-auto w-full py-1.5 file:me-2 file:rounded-md file:border-0 file:bg-secondary file:px-2 file:py-1 file:text-sm`}
                  onChange={(event) => void chooseImage(event.target.files?.[0])}
                />
              )}
            </Field>
            {image ? (
              <div className="flex items-center gap-3">
                <img
                  src={image.dataUrl}
                  alt={copy.image.preview}
                  className="max-h-20 rounded-md border border-input bg-background p-1"
                />
                <Button type="button" variant="outline" size="sm" onClick={() => setImage(null)}>
                  {copy.image.clear}
                </Button>
              </div>
            ) : null}
          </TabsContent>
        </Tabs>
        {modeError && !imageError ? (
          <p className="text-xs text-muted-foreground text-start">{modeError}</p>
        ) : null}
      </section>

      <PlacementFields
        draft={placementDraft}
        onChange={setPlacementDraft}
        errors={parsed.errors}
        placement={parsed.placement}
        disabled={disabled}
      />

      <fieldset className="grid gap-3 border-0 p-0" disabled={disabled}>
        <legend className={sectionTitleClass}>{copy.details.title}</legend>
        <Field label={copy.details.signer}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="text"
              autoComplete="off"
              className={`${controlClass} w-full`}
              value={signer}
              onChange={(event) => setSigner(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.details.color} error={validation.errors.color}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="color"
              className="h-9 w-16 cursor-pointer rounded-lg border border-input bg-background p-1"
              value={color}
              onChange={(event) => setColor(event.target.value)}
            />
          )}
        </Field>
      </fieldset>
    </>
  );
}

const LEVELS = [
  { value: "B-B", label: signCopy.level.bb },
  { value: "B-T", label: signCopy.level.bt },
  { value: "B-LT", label: signCopy.level.blt },
  { value: "B-LTA", label: signCopy.level.blta },
] as const;

/** `pdf.sign`: the cryptographic PAdES signature, with the certificate held in memory only. */
function SignStudio({ tool, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const copy = signCopy;
  const [certificate, setCertificate] = useState<CertificateDraft>(EMPTY_CERTIFICATE);
  const [visible, setVisible] = useState(false);
  const [placementDraft, setPlacementDraft] = useState<PlacementDraft>(DEFAULT_PLACEMENT);
  const [appearance, setAppearance] = useState("");
  const [signer, setSigner] = useState("");
  const [reason, setReason] = useState("");
  const [location, setLocation] = useState("");
  const [contact, setContact] = useState("");
  const [fieldName, setFieldName] = useState("");
  const [level, setLevel] = useState<(typeof LEVELS)[number]["value"]>("B-B");
  const [tsa, setTsa] = useState("");

  const parsed = parsePlacement(placementDraft, { pageCount, wholeNumbers: true });
  const certErrors = certificateErrors(certificate);
  const needsTsa = level !== "B-B";

  // The passphrase and certificate bytes never enter these params; they carry prompt references.
  const params = useMemo<Record<string, unknown>>(
    () => ({
      certificate: PROMPT,
      password: PROMPT,
      level,
      ...optional({
        reason: text(reason),
        location: text(location),
        contact: text(contact),
        field_name: text(fieldName),
        tsa_url: text(tsa),
      }),
      ...(visible && parsed.placement
        ? {
            visible: {
              ...parsed.placement,
              ...optional({ appearance: text(appearance) ?? text(signer) }),
            },
          }
        : {}),
    }),
    [level, reason, location, contact, fieldName, tsa, visible, placementDraft, appearance, signer],
  );
  const schemaParams = useMemo(() => ({ ...params, certificate: null, password: null }), [params]);
  const validation = useEmitParams(tool, params, {
    schemaParams,
    extraErrors: {
      ...certErrors,
      ...(visible && parsed.errors.page ? { visible: parsed.errors.page } : {}),
      ...(visible && parsed.errors.rect ? { visible: parsed.errors.rect } : {}),
      ...(needsTsa && !tsa.trim() ? { tsa_url: copy.level.tsaRequired } : {}),
    },
    onChange,
  });

  return (
    <>
      <CertificateInput
        draft={certificate}
        onChange={setCertificate}
        errors={certErrors}
        disabled={disabled}
      />

      <fieldset className="grid gap-3 border-0 p-0" disabled={disabled}>
        <legend className={sectionTitleClass}>{copy.details.title}</legend>
        <Field label={copy.details.signer}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="text"
              autoComplete="off"
              className={`${controlClass} w-full`}
              value={signer}
              onChange={(event) => setSigner(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.details.reason}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="text"
              autoComplete="off"
              className={`${controlClass} w-full`}
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.details.location}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="text"
              autoComplete="off"
              className={`${controlClass} w-full`}
              value={location}
              onChange={(event) => setLocation(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.details.contact}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="text"
              autoComplete="off"
              className={`${controlClass} w-full`}
              value={contact}
              onChange={(event) => setContact(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.details.fieldName}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="text"
              autoComplete="off"
              placeholder="Signature1"
              className={`${controlClass} w-full`}
              value={fieldName}
              onChange={(event) => setFieldName(event.target.value)}
            />
          )}
        </Field>
      </fieldset>

      <div className="grid gap-1.5">
        <p className={sectionTitleClass}>{copy.level.label}</p>
        <ChoiceChips
          label={copy.level.label}
          options={LEVELS}
          value={level}
          onSelect={setLevel}
          disabled={disabled}
        />
        {needsTsa ? (
          <Field label={copy.level.tsa} hint={copy.level.tsaHint} error={validation.errors.tsa_url}>
            {({ id, invalid: _invalid, ...aria }) => (
              <input
                {...aria}
                id={id}
                type="url"
                autoComplete="off"
                spellCheck={false}
                className={`${controlClass} w-full`}
                disabled={disabled}
                value={tsa}
                onChange={(event) => setTsa(event.target.value)}
              />
            )}
          </Field>
        ) : null}
      </div>

      <div className="grid gap-3">
        <CheckRow
          label={copy.visible.label}
          hint={copy.visible.hint}
          checked={visible}
          onChange={setVisible}
          disabled={disabled}
        />
        {visible ? (
          <>
            <PlacementFields
              draft={placementDraft}
              onChange={setPlacementDraft}
              errors={parsed.errors}
              placement={parsed.placement}
              disabled={disabled}
            />
            <Field label={copy.details.appearanceText} hint={copy.details.appearanceHint}>
              {({ id, invalid: _invalid, ...aria }) => (
                <input
                  {...aria}
                  id={id}
                  type="text"
                  autoComplete="off"
                  className={`${controlClass} w-full`}
                  disabled={disabled}
                  value={appearance}
                  onChange={(event) => setAppearance(event.target.value)}
                />
              )}
            </Field>
            <p className="text-xs text-muted-foreground text-start">{copy.visible.drawnNote}</p>
          </>
        ) : null}
      </div>
    </>
  );
}

function listOf(value: string): string[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

/** `pdf.sign_verify`: settings for the check, and the card the result is shown in. */
function VerifyStudio({ tool, disabled, locale, onChange }: CustomToolPanelProps) {
  const copy = signCopy.verify;
  const [fetchRevocation, setFetchRevocation] = useState(false);
  const [tsa, setTsa] = useState("");
  const [anchors, setAnchors] = useState("");
  const [evidence, setEvidence] = useState("");
  const params = useMemo<Record<string, unknown>>(
    () => ({
      fetch_revocation: fetchRevocation,
      trust_anchors: listOf(anchors),
      offline_evidence: listOf(evidence),
      ...optional({ tsa_url: text(tsa) }),
    }),
    [fetchRevocation, anchors, evidence, tsa],
  );
  useEmitParams(tool, params, { onChange });
  return (
    <>
      <fieldset className="grid gap-3 border-0 p-0" disabled={disabled}>
        <legend className={sectionTitleClass}>{copy.title}</legend>
        <CheckRow
          label={copy.fetchRevocation}
          hint={copy.fetchRevocationHint}
          checked={fetchRevocation}
          onChange={setFetchRevocation}
        />
        <Field label={copy.tsa}>
          {({ id, invalid: _invalid, ...aria }) => (
            <input
              {...aria}
              id={id}
              type="url"
              autoComplete="off"
              spellCheck={false}
              className={`${controlClass} w-full`}
              value={tsa}
              onChange={(event) => setTsa(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.anchors} hint={copy.anchorsHint}>
          {({ id, invalid: _invalid, ...aria }) => (
            <textarea
              {...aria}
              id={id}
              spellCheck={false}
              className={`${controlClass} min-h-20 w-full py-2 font-mono text-xs`}
              value={anchors}
              onChange={(event) => setAnchors(event.target.value)}
            />
          )}
        </Field>
        <Field label={copy.evidence} hint={copy.evidenceHint}>
          {({ id, invalid: _invalid, ...aria }) => (
            <textarea
              {...aria}
              id={id}
              spellCheck={false}
              className={`${controlClass} min-h-20 w-full py-2 font-mono text-xs`}
              value={evidence}
              onChange={(event) => setEvidence(event.target.value)}
            />
          )}
        </Field>
      </fieldset>
      <SignatureValidationCard locale={locale} />
    </>
  );
}

/** `pdf.sign_remove`: which signature to strip, behind an explicit confirmation. */
function RemoveStudio({ tool, disabled, onChange }: CustomToolPanelProps) {
  const copy = signCopy.remove;
  const [field, setField] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const params = useMemo<Record<string, unknown>>(
    () => ({ confirm_invalidation: confirmed, ...optional({ field_name: text(field) }) }),
    [confirmed, field],
  );
  const validation = useEmitParams(tool, params, {
    extraErrors: confirmed ? {} : { confirm_invalidation: copy.confirmRequired },
    onChange,
  });
  return (
    <fieldset className="grid gap-3 border-0 p-0" disabled={disabled}>
      <legend className={sectionTitleClass}>{copy.title}</legend>
      <Field label={copy.field} hint={copy.fieldHint}>
        {({ id, invalid: _invalid, ...aria }) => (
          <input
            {...aria}
            id={id}
            type="text"
            autoComplete="off"
            className={`${controlClass} w-full`}
            value={field}
            onChange={(event) => setField(event.target.value)}
          />
        )}
      </Field>
      <Alert variant="destructive">
        <TriangleAlertIcon aria-hidden="true" />
        <AlertDescription>{copy.confirm}</AlertDescription>
      </Alert>
      <CheckRow label={copy.confirm} checked={confirmed} onChange={setConfirmed} />
      <ErrorText>{validation.errors.confirm_invalidation}</ErrorText>
    </fieldset>
  );
}

/** Purpose-built studio for the signing tools; each tool remounts its own draft. */
export function SignPanel(props: CustomToolPanelProps) {
  const { id } = props.tool;
  return (
    <div key={id} data-slot="sign-panel" data-tool={id} className="grid gap-4 text-start">
      {id === "pdf.sign_visual" ? (
        <VisualStudio {...props} />
      ) : id === "pdf.sign" ? (
        <SignStudio {...props} />
      ) : id === "pdf.sign_verify" ? (
        <VerifyStudio {...props} />
      ) : (
        <RemoveStudio {...props} />
      )}
    </div>
  );
}
