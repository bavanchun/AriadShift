"use client";
// Loaded on demand by the app shell: the tool hub with its command palette and the tool registry.
import { ToolHub, type ToolHubProps } from "../tools/hub";
import { useToolsTranslate } from "../tools/use-tools-translate";

export default function ToolsHost(props: Omit<ToolHubProps, "translate">) {
  const translate = useToolsTranslate();
  return <ToolHub {...props} translate={translate} />;
}
