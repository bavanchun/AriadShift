import type { ToolDescriptor } from "@/converter/tool-catalog";

/** A short display name derived from the id, `pdf.delete_pages` → "Delete pages". */
export function toolLabel(tool: Pick<ToolDescriptor, "id">): string {
  const name = tool.id.split(".").slice(1).join(" ") || tool.id;
  const words = name.replaceAll("_", " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
