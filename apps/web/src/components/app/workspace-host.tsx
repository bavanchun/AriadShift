"use client";
// Loaded on demand by the app shell: it pulls in the document workspace, the viewer adapter and,
// through it, the EmbedPDF modules, so none of that is part of the app route's first load.
import { useEffect, useMemo } from "react";
import { ConverterClient } from "@/converter/client";
import { DocumentWorkspace } from "../workspace/document-workspace";
import { useWorkspaceIntl } from "../workspace/use-workspace-copy";

export default function WorkspaceHost({
  requestedTool,
}: {
  requestedTool: { id: string; nonce: number } | null;
}) {
  // The workspace owns its converter worker, apart from the converter tab's.
  const client = useMemo(() => new ConverterClient(), []);
  useEffect(() => () => client.terminate(), [client]);
  const { copy, viewerLabels } = useWorkspaceIntl();
  return (
    <DocumentWorkspace
      client={client}
      onProgress={(listener) => client.onProgress(listener)}
      copy={copy}
      viewerLabels={viewerLabels}
      requestedTool={requestedTool}
    />
  );
}
