"use client";

import { useState } from "react";
import { ocrCopy } from "./copy";
import { Chip } from "./panel-kit";

const ENGLISH = ["Service agreement no. 042", "Signed on 12 March 2026", "Party A: Ariad Ltd."];

const SAMPLES: Record<string, string[]> = {
  vie: ["Hợp đồng dịch vụ số 042", "Ngày ký: 12 tháng 3 năm 2026", "Bên A: Công ty Ariad"],
  eng: ["Service agreement no. 042", "Signed on 12 March 2026", "Party A: Ariad Ltd."],
  fra: ["Contrat de service n° 042", "Signé le 12 mars 2026", "Partie A : Ariad SARL"],
  deu: ["Dienstleistungsvertrag Nr. 042", "Unterzeichnet am 12. März 2026", "Partei A: Ariad GmbH"],
  spa: ["Contrato de servicio n.º 042", "Firmado el 12 de marzo de 2026", "Parte A: Ariad S.L."],
  jpn: ["サービス契約書 第042号", "締結日：2026年3月12日", "甲：Ariad株式会社"],
  kor: ["서비스 계약서 제042호", "체결일: 2026년 3월 12일", "갑: Ariad 주식회사"],
};

/** A diagram, not a render: it explains what the recogniser adds to a scan. */
export function TextLayerPreview({
  languages,
  deskew,
  clean,
}: {
  languages: readonly string[];
  deskew: boolean;
  clean: boolean;
}) {
  const [view, setView] = useState<"overlay" | "plain">("overlay");
  const copy = ocrCopy.preview;
  const lines = SAMPLES[languages[0] ?? "eng"] ?? ENGLISH;
  return (
    <section
      data-slot="ocr-text-layer-preview"
      aria-label={copy.title}
      className="grid gap-3 rounded-xl border border-input bg-card p-3 text-start"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">{copy.title}</h3>
        {/* biome-ignore lint/a11y/useSemanticElements: a labelled group of toggle buttons */}
        <div role="group" aria-label={copy.mode} className="flex gap-1.5">
          <Chip pressed={view === "overlay"} onClick={() => setView("overlay")}>
            {copy.overlay}
          </Chip>
          <Chip pressed={view === "plain"} onClick={() => setView("plain")}>
            {copy.plain}
          </Chip>
        </div>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <figure className="grid gap-1.5" data-slot="ocr-scan-image">
          <div
            aria-hidden="true"
            className={`grid gap-2 rounded-lg border border-input p-3 ${clean ? "bg-white" : "bg-stone-200"}`}
            style={{ transform: deskew ? "none" : "rotate(-1.5deg)" }}
          >
            {lines.map((line) => (
              <div
                key={line}
                className="h-2.5 rounded-sm bg-stone-500/70"
                style={{ width: `${55 + (line.length % 5) * 8}%` }}
              />
            ))}
          </div>
          <figcaption className="text-xs text-muted-foreground">{copy.scanned}</figcaption>
        </figure>
        <figure className="grid gap-1.5" data-slot="ocr-text-layer" data-view={view}>
          <div
            className={`relative rounded-lg border border-input p-3 ${view === "overlay" ? "bg-stone-200" : "bg-background"}`}
          >
            <ul className="grid gap-1.5 text-sm" lang={languages[0]}>
              {lines.map((line) => (
                <li
                  key={line}
                  className={
                    view === "overlay"
                      ? "rounded-sm bg-primary/10 text-transparent selection:bg-primary/30 selection:text-foreground"
                      : "text-foreground"
                  }
                >
                  {line}
                </li>
              ))}
            </ul>
          </div>
          <figcaption className="text-xs text-muted-foreground">
            {copy.layer} · {copy.sample(languages.length || 1)}
          </figcaption>
        </figure>
      </div>
      <p className="text-xs text-muted-foreground">
        {view === "overlay" ? copy.overlayNote : copy.plainNote}
      </p>
    </section>
  );
}
