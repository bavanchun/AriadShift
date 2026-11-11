"use client";

import { resolvePageCount } from "../organiser/panel-kit";
import type { CustomToolPanelProps } from "../types";
import { BookletStudio } from "./booklet-studio";
import { CropStudio } from "./crop-studio";
import { NupStudio } from "./nup-studio";
import { ScaleStudio } from "./scale-studio";
import { SinglePageStudio } from "./single-page-studio";

export const IMPOSITION_TOOLS = [
  "pdf.nup",
  "pdf.booklet",
  "pdf.scale",
  "pdf.single_page",
  "pdf.crop",
] as const;

export type ImpositionToolId = (typeof IMPOSITION_TOOLS)[number];

export function isImpositionTool(id: string): id is ImpositionToolId {
  return (IMPOSITION_TOOLS as readonly string[]).includes(id);
}

/** Purpose-built studio for the layout tools, each with a live drawing of its result. */
export function ImpositionPanel(props: CustomToolPanelProps) {
  const shared = { ...props, pageCount: resolvePageCount(props.pageCount, props.files) };
  const { id } = props.tool;
  let studio: React.ReactNode = null;
  switch (id) {
    case "pdf.nup":
      studio = <NupStudio {...shared} />;
      break;
    case "pdf.booklet":
      studio = <BookletStudio {...shared} />;
      break;
    case "pdf.scale":
      studio = <ScaleStudio {...shared} />;
      break;
    case "pdf.single_page":
      studio = <SinglePageStudio {...shared} />;
      break;
    case "pdf.crop":
      studio = <CropStudio {...shared} />;
      break;
  }
  return (
    <div key={id} data-slot="imposition-panel" data-tool={id} className="grid gap-4 text-start">
      {studio}
    </div>
  );
}
