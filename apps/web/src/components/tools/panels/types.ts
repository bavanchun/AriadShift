import type { ParamsValidation } from "@/components/tools/params-form";
import type { WorkspaceFile } from "@/components/workspace/types";
import type { ToolDescriptor } from "@/converter/tool-catalog";

/**
 * Props every purpose-built tool panel receives. A panel owns its draft parameters and reports
 * each change, with the validation the generic form would have produced, through `onChange`.
 */
export interface CustomToolPanelProps {
  tool: ToolDescriptor;
  /** The files the tool will read, in the order the workspace passes them to the engine. */
  files: WorkspaceFile[];
  /** Page count of the file the tool reads, for page-range validation and previews. */
  pageCount?: number;
  disabled?: boolean;
  locale?: string;
  onChange: (params: Record<string, unknown>, validation: ParamsValidation) => void;
}
