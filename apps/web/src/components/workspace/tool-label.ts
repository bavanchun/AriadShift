"use client";
import { useCallback } from "react";
import { useWorkspaceCopy } from "./copy";

/** The display name of a tool in the active locale, for example "Delete pages" for `pdf.delete_pages`. */
export function useToolLabel(): (tool: { id: string }) => string {
  const { title } = useWorkspaceCopy().tools;
  return useCallback((tool) => title(tool.id), [title]);
}
