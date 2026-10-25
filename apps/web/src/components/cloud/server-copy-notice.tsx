"use client";

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { LoaderCircleIcon, TriangleAlertIcon } from "lucide-react";
import { useState } from "react";
import type { DiscardOutcome, ServerCopy } from "@/lib/cloud/cloud-job";
import { formatClock } from "@/lib/cloud/steps";

export interface ServerCopyNoticeProps {
  copy: ServerCopy;
  host: string;
  /** Called with the outcome of "Delete from server"; a kept copy replaces this one. */
  onSettled: (copy: ServerCopy, outcome: DiscardOutcome) => void;
}

/**
 * Names a file that is still on the server after "Start over" or after leaving
 * cloud mode, says when retention deletes it, and offers the deletion again.
 */
export function ServerCopyNotice({ copy, host, onSettled }: ServerCopyNoticeProps) {
  const [deleting, setDeleting] = useState(false);
  const retention = copy.expiresAt
    ? `${host} deletes it automatically at ${formatClock(copy.expiresAt)}.`
    : `${host} deletes it automatically when its retention period ends.`;

  const retry = async () => {
    setDeleting(true);
    const outcome = await copy.retry();
    setDeleting(false);
    onSettled(copy, outcome);
  };

  return (
    <Alert variant="destructive" data-testid="server-copy">
      <TriangleAlertIcon aria-hidden="true" />
      <AlertTitle>{copy.fileName} is still on {host}</AlertTitle>
      <AlertDescription>
        <p>
          {copy.reason === "unsupported"
            ? "This server cannot delete an upload on request."
            : "Deleting it from the server failed or the server was unreachable."}{" "}
          {retention}
        </p>
        {copy.reason === "failed" && (
          <Button
            variant="outline"
            size="lg"
            className="mt-2"
            onClick={() => void retry()}
            disabled={deleting}
          >
            {deleting && <LoaderCircleIcon aria-hidden="true" className="animate-spin" />}
            Delete from server
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}
