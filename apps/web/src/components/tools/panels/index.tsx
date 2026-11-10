"use client";
// One entry point from a tool to its purpose-built studio panel. A tool outside every set keeps
// the generic schema form, so no tool is ever left without a way to fill in its parameters.
import { ParamsForm } from "../params-form";
import { ComparePanel, isCompareTool } from "./compare";
import { CompressPanel, isCompressTool } from "./compress";
import { ImpositionPanel, isImpositionTool } from "./imposition";
import { isOcrTool, OcrPanel } from "./ocr";
import { isOrganiserTool, OrganiserPanel } from "./organiser";
import { isRedactTool, RedactPanel } from "./redact";
import { isSecurityTool, SecurityPanel } from "./security";
import { isSignTool, SignPanel } from "./sign";
import type { CustomToolPanelProps } from "./types";
import { isWatermarkTool, WatermarkPanel } from "./watermark";

export { RedactionVerificationCard, type RedactionVerificationReport } from "./redact";
export { SignatureValidationCard, type SignValidationReport } from "./sign";
export type { CustomToolPanelProps } from "./types";

export interface ToolPanelProps extends CustomToolPanelProps {
  /** Reports the input files in run order when a panel reorders them (the compare swap). */
  onInputsChange?: (ids: string[]) => void;
}

/** Whether the tool has its own studio rather than the generic parameter form. */
export function hasCustomPanel(toolId: string): boolean {
  return (
    isOrganiserTool(toolId) ||
    isImpositionTool(toolId) ||
    isCompressTool(toolId) ||
    isSecurityTool(toolId) ||
    isWatermarkTool(toolId) ||
    isOcrTool(toolId) ||
    isRedactTool(toolId) ||
    isSignTool(toolId) ||
    isCompareTool(toolId)
  );
}

/** Renders the studio for `tool`, or the generic parameter form when it has none. */
export function ToolPanel({ onInputsChange, ...panel }: ToolPanelProps) {
  const { tool } = panel;
  if (isOrganiserTool(tool.id)) return <OrganiserPanel {...panel} />;
  if (isImpositionTool(tool.id)) return <ImpositionPanel {...panel} />;
  if (isCompressTool(tool.id)) return <CompressPanel {...panel} />;
  if (isSecurityTool(tool.id)) return <SecurityPanel {...panel} />;
  if (isWatermarkTool(tool.id)) return <WatermarkPanel {...panel} />;
  if (isOcrTool(tool.id)) return <OcrPanel {...panel} />;
  if (isRedactTool(tool.id)) return <RedactPanel {...panel} />;
  if (isSignTool(tool.id)) return <SignPanel {...panel} />;
  if (isCompareTool(tool.id)) return <ComparePanel {...panel} onInputsChange={onInputsChange} />;
  return (
    <ParamsForm
      tool={tool}
      pageCount={panel.pageCount}
      inputs={panel.files.map((file) => ({ id: file.id, name: file.name }))}
      disabled={panel.disabled}
      onChange={panel.onChange}
    />
  );
}
