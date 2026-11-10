"use client";

import type { CustomToolPanelProps } from "../types";
import { InsertBlankStudio } from "./insert-blank-studio";
import { AlternateMixStudio, MergeStudio } from "./multi-file-studio";
import { PageSelectionStudio, RotateStudio } from "./page-pages-studio";
import { resolvePageCount } from "./panel-kit";
import { ReorderStudio } from "./reorder-studio";
import { SplitStudio } from "./split-studio";

export const ORGANISER_TOOLS = [
  "pdf.merge",
  "pdf.split",
  "pdf.extract_pages",
  "pdf.delete_pages",
  "pdf.reorder",
  "pdf.rotate",
  "pdf.insert_blank",
  "pdf.alternate_mix",
] as const;

export type OrganiserToolId = (typeof ORGANISER_TOOLS)[number];

export function isOrganiserTool(id: string): id is OrganiserToolId {
  return (ORGANISER_TOOLS as readonly string[]).includes(id);
}

/** Purpose-built studio for the page-organising tools; each tool remounts its own draft. */
export function OrganiserPanel(props: CustomToolPanelProps) {
  const pageCount = resolvePageCount(props.pageCount, props.files);
  const shared = { ...props, pageCount };
  const { id } = props.tool;
  let studio: React.ReactNode = null;
  switch (id) {
    case "pdf.merge":
      studio = <MergeStudio {...shared} />;
      break;
    case "pdf.alternate_mix":
      studio = <AlternateMixStudio {...shared} />;
      break;
    case "pdf.split":
      studio = <SplitStudio {...shared} />;
      break;
    case "pdf.extract_pages":
    case "pdf.delete_pages":
      studio = <PageSelectionStudio {...shared} />;
      break;
    case "pdf.reorder":
      studio = <ReorderStudio {...shared} />;
      break;
    case "pdf.rotate":
      studio = <RotateStudio {...shared} />;
      break;
    case "pdf.insert_blank":
      studio = <InsertBlankStudio {...shared} />;
      break;
  }
  return (
    <div key={id} data-slot="organiser-panel" data-tool={id} className="grid gap-4 text-start">
      {studio}
    </div>
  );
}
