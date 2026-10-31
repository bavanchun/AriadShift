# CloudConvert job UX study and cloud-mode requirements for `/app`

Date: 2026-10-25. Subject: the public `cloudconvert.com` conversion flow, used as an anonymous visitor. Audience: the workers building web cloud mode (`apps/web/src/components/cloud/**`, `apps/web/src/lib/cloud/**`), the jobs page and `docs/api.md`.

## Outcome

CloudConvert's flow is fast and clear on the happy path. It is weak in exactly the places where AriadShift wants to stand out: it never says near the drop zone that the file leaves the device or where it goes, it never shows the retention time in the flow, its delete button removes server files without saying so, and failures show raw engine output while the live region announces "1 file ready to convert". The requirements below keep CloudConvert's good patterns (staged files, one row per file, a polite status line, a determinate upload bar) and add an explicit destination choice, a visible expiry, a confirmed deletion, plain-language errors, and correct announcements.

## Method

- Tool: the REA MCP tools (`mcp__rea__*`) and the `reverse-engineer-anything` skill are not installed on this machine, so the study used `agent-browser` 0.39.1 (Chromium over CDP) from the `ak:agent-browser` skill, in its own named session. Captures were DOM snapshots (accessibility tree), a `MutationObserver` timeline of the visible text, a wrapped `WebSocket` constructor that recorded only host names, the network log, and screenshots at 1280×900 and 390×844.
- Inputs were synthetic files created for the study: `hello.md` (41 B, one heading and one line), `synthetic-3mb.md` (3.4 MB of numbered filler lines, to make upload progress visible) and `broken.png` (17 B of text with a `.png` name, to trigger a conversion failure). No private document was uploaded.
- No account was created, nothing was paid, and no limit was tested by exceeding it. Limits come from the public pricing page. Six conversions were run, below the free allowance of 10 per day. Three jobs were deleted with the in-page delete button; the other three expire on CloudConvert's 24-hour schedule.
- Screenshots stayed in the session scratchpad and are not committed; they are described in text below.

## Observed flow

### Entry, file pick and format selection

- The route page (`/md-to-docx`) shows a heading, a format pair (`MD` to `DOCX`, each a button that opens a format popover) and a card: "Select your file to convert / or drop your file here." with a split button: "Select File" plus a chevron "Select file source" menu (From my computer, By URL, From Google Drive, From Dropbox, From OneDrive).
- Nothing on the card or near it says the file is uploaded, to which region, or for how long. The only disclosures are the footer links "Security" and "Privacy".
- Picking a file stages it locally. Nothing is uploaded at this point; the only request is the operations lookup for the format pair. The row shows `hello.md`, `41 B · MD Document`, "Convert", the source format chip, an arrow, an output format dropdown, and an icon-only delete (✕). A sticky footer shows "1 file ready to convert" and a primary "Convert" button.
- A file whose extension does not match the route moves the page to the matching route (`broken.png` turned `/md-to-docx` into `/png-converter`) and leaves the output as "Select Format", with "Please select output format" in the footer and a disabled Convert button.
- The format popover is a `dialog`. Focus moves into a "Search Format" text box, category buttons (Document, Image) filter the list, and each format is a button. Typing "pd" leaves only PDF. A search with no match shows an empty list with no "no formats match" message. Escape closes the popover and returns focus to its trigger. The selected format is not marked with `aria-pressed` or `aria-current`.

### Upload and job

Network shape on Convert: `GET /session`, then `POST api.cloudconvert.com/v2/jobs` with three tasks (`import/upload`, `convert` with engine `libreoffice`, `export/url`), then a presigned form `POST` to `eu-central.storage.cloudconvert.com/tasks` with `acl: private`. The job response names `region: eu-central` and storage `ceph-fra` (Frankfurt) and gives anonymous jobs `priority: -10`. Status reaches the page over a socket.io WebSocket (`socketio.cloudconvert.com`); there is no polling.

Visible-text timeline for the 3.4 MB file (milliseconds after the page script started observing):

| t (ms) | Row text | Footer |
|---:|---|---|
| 208 | `synthetic-3mb.md · 3.3 MB · MD Document` | 1 file ready to convert |
| 1474 | Waiting... · Uploading | Processing 1 of 1… |
| 1563–3067 | Waiting... 2% → 75% · Uploading, with a `progressbar` (`aria-valuenow`, `aria-label` = "2%") | Processing 1 of 1… |
| 3270 | Waiting... (bar becomes indeterminate) · Uploading | Processing 1 of 1… |
| 3938 | Waiting... | Processing 1 of 1… |
| 4025 | Processing... (indeterminate, no elapsed time) | Processing 1 of 1… |
| 20021 | FINISHED · `synthetic-3mb.docx · 185 KB` · Download | Done |

The 41-byte file went through the same states in about 2.7 seconds. During upload the row's main label says "Waiting..." while a dim right-aligned word says "Uploading", so the strongest text describes the wrong state. Processing lasted 16 seconds with no stage, estimate or elapsed time. There is no cancel control other than the ✕.

### Download, delete and retention

- Finished rows show a green `FINISHED` badge, the output name and size, and a "Download" link straight to a presigned storage URL (no `download` attribute, same tab). The link's accessible name is "Download", without the file name.
- The ✕ on a finished row sends `DELETE /v2/jobs/{id}` (204) and the row disappears. There is no confirmation, no toast and no "deleted from our servers" message; the notifications region stays empty and focus falls to `<body>`. The only place that explains the ✕ is the privacy policy ("You can manually delete files from our servers when you click the '×' button").
- Retention is stated only in the privacy policy: files are deleted when the delete button is used and "automatically at the latest after 24 hours". The security page says "Files are kept only for processing and deleted immediately afterwards", which reads differently from the 24-hour policy. Neither statement appears in the conversion flow, and the finished row shows no expiry time.

### Errors and limits

- Corrupt input (`broken.png` → JPG): the row shows a red `FAILED` badge and the raw ImageMagick message ``convert: no images defined `/output/broken.jpg' @ error/deprecate.c/ConvertImageCommand/3375.`` with a "Show details" / "Hide details" toggle. The input size disappears from the row, the footer returns to "1 file ready to convert" and Convert is enabled again, which retries the job implicitly. The polite live region announces "1 file ready to convert", not the failure.
- Offline before upload: the row shows `FAILED · Failed to start job`; the footer again says "1 file ready to convert". There is no hint to check the connection and no explicit retry button.
- Missing output format: the footer says "Please select output format" (polite live region) and Convert is disabled with the native `disabled` attribute, so keyboard users cannot reach it to learn why.
- Free-tier limits from the pricing page: 10 conversions per day with a daily reset, 1 GB maximum file size, 5 minutes maximum processing time, 5 concurrent tasks, low processing priority. None of these is shown in the flow before a limit is hit; this study did not trigger one.

### Accessibility

- `lang="en"` and a `main` landmark are present. There is no skip link; seven tab stops (logo, Tools, API, Pricing, Sign in, Sign up, theme) come before the first converter control.
- The drop card is itself a button ("Select file") and is followed by a second "Select File" button, so keyboard users meet two stops that do the same thing.
- The Tools, API and Pricing items compute `outline: none` with no box shadow when focused, so their focus indicator, if any, is weak. Converter controls use a focus shadow or a 3 px outline.
- Row controls are named without the file: "Delete", "Download", and the output dropdown is named "DOCX". With several files these names are ambiguous.
- After Convert, focus is dropped to `<body>`. After delete it is also dropped to `<body>`.
- The upload `progressbar` has a value but its label is only the percentage, not what is uploading.
- At 390 px width the layout reflows without horizontal scroll; the output size is truncated (`hello.docx · 5.6…`), the Download target is 113×32 px, and the ✕ sits at the top right of the card, far from the result it deletes.

## Annotated screenshots

1. **Landing, 1280×900 (`01-landing.png`).** Dark hero with "Markdown to Word Converter" and the `MD → DOCX` format pair at the right. The white card below holds the cloud-upload icon, "Select your file to convert", "or drop your file here." and the red "Select File" split button. Annotation: the card is the only call to action, and nothing on it says the file will be uploaded, to Frankfurt, for up to 24 hours. AriadShift's equivalent card must carry the destination choice.
2. **Staged file (`04-after-select-3.png`).** One row: file icon, `hello.md`, `41 B · MD Document`, then "Convert · MD → DOCX ▾ · ✕". The sticky footer reads "✓ 1 file ready to convert" next to "Convert with AI", "Add more files ▾" and the red "Convert" button. Annotation: staging before upload is a good pattern because nothing leaves the device until the user commits; AriadShift should keep it and put the destination and expiry next to the commit button.
3. **Uploading 3.4 MB (`06-big-3.png`).** The row gains a second line: "Waiting... **2%**" at the left, a thin red bar, and a dim "Uploading" at the right. The footer shows a spinner and "Processing 1 of 1…". Annotation: three labels disagree (Waiting, Uploading, Processing). AriadShift must name the current step once, in the main label, and keep the footer as a summary.
4. **Finished (`05-job-now.png`).** Green `FINISHED` badge, `hello.docx · 5.6 KB`, black "Download" button; footer "✓ Done". Annotation: no expiry, no "delete now" text, and the ✕ above deletes server files with no label explaining it. AriadShift must show "Deletes at 19:45 (in 60 min)" and a labelled "Delete now" here.
5. **Failed (`11-error-details.png`).** Red `FAILED` badge followed by the raw ImageMagick command error and "Hide details"; the footer has returned to "✓ 1 file ready to convert" with Convert enabled. Annotation: the user learns nothing actionable, the success-style check mark contradicts the failure, and screen readers hear only "1 file ready to convert". AriadShift must map `ErrorDetail.code` to plain copy with a next step and announce failures assertively.

## Requirements for AriadShift cloud mode

Tags: **MUST** is required for the phase acceptance; **SHOULD** is expected unless a concrete reason is recorded. "More explicit than CloudConvert" marks the private-by-default differentiators. The states and codes refer to `schemas/openapi.v1.json` (`UploadStatus`, `JobStatus`, `JobEvent.kind`, `ErrorDetail.code`) and the plan table in `docs/server.md`.

### Destination choice and privacy disclosure

- **MUST** Default `/app` to on-device conversion. Cloud is a choice the user makes for this file, never a silent fallback. A route that needs the server (for example a Docling or Office route) shows the choice instead of uploading. *More explicit than CloudConvert*, which has no local mode and no choice.
- **MUST** Show the destination next to the commit button before anything leaves the device, with the server origin, what is sent, how long it is kept and how to delete it. Copy: "This file will be uploaded to **{server host}** for conversion. It is deleted automatically **1 hour** after upload, or sooner when you choose Delete now." Take the host from the configured API base URL and the retention from the server (`Upload.expires_at` / plan `retention_s`), never from a hard-coded string. *More explicit than CloudConvert*, which puts this only in the privacy policy.
- **MUST** Require an explicit action for each cloud upload: the commit button reads "Upload and convert", not "Convert", while the cloud destination is selected. A per-session "remember for this tab" option is acceptable; a persistent silent default is not.
- **MUST** Say what the server keeps beyond the file, in one line under the disclosure: "The file name is stored with the job and never logged. Files are not used for anything other than this conversion." Match the server's actual behaviour (`CreateUploadRequest.filename` is "never logged") and the cloud privacy statement that `docs/api.md` will publish; link to it.
- **SHOULD** For self-hosted deployments, state who operates the server ("Operated by {operator}" when configured, otherwise the host name only). Do not claim a region or certification the deployment cannot prove. *More explicit than CloudConvert*, whose region appears only in API responses.
- **SHOULD** Keep one statement of retention across the flow, the docs and the privacy page. CloudConvert's "deleted immediately afterwards" versus "at the latest after 24 hours" is the inconsistency to avoid.

### File pick and format selection

- **MUST** Stage picked files locally and upload only on the explicit commit (CloudConvert's good pattern). Show name, size and detected format per row.
- **MUST** Use one native file input behind one visible "Choose file" button. The drop zone is a pointer convenience, not a second keyboard stop.
- **MUST** Check the plan limits before upload, from the server's plan data: "Anonymous uploads are limited to 10 MB. This file is 14.2 MB." Do not start an upload the server will refuse with `413 payload_too_large` or `limit_exceeded`.
- **SHOULD** Keep the current URL and target when the picked file does not match a pair page; preselect the new source and ask for a target, rather than navigating to another route as CloudConvert does.
- **SHOULD** Mark the selected target in the format picker (`aria-pressed="true"` on a button list, or `aria-selected` in a listbox) and show "No formats match '{query}'" in a polite live region for an empty search.

### Upload progress

- **MUST** Show a determinate progress bar during the presigned `PUT`, fed by upload progress events, with an accessible name that says what is happening: `aria-label="Uploading report.pdf"` and `aria-valuetext="45 percent, 4.1 of 9.0 MB"`.
- **MUST** Name the step once, in the row's main label: "Uploading 45%". Do not show "Waiting" while bytes are moving.
- **MUST** Offer "Cancel upload" while uploading. Cancelling aborts the transfer and confirms what happens to partial data. With today's API (no upload delete) the copy is "Upload canceled. Any partial data is deleted automatically at {expiry}."; once an upload delete exists, call it and say "Upload canceled. Nothing was kept." (see Unresolved questions).
- **SHOULD** Show "Checking file…" between the end of the transfer and `POST /v1/uploads/{id}/complete`, since the server verifies `sha256` and size there.

### Job progress model

Map server states to a short, fixed set of user-facing steps. The SSE stream (`/v1/jobs/{id}/events`, resumable with `Last-Event-ID`) drives the transitions; `GET /v1/jobs/{id}` is the fallback when the stream drops.

| Server signal | Step label | Indicator | Announcement |
|---|---|---|---|
| upload `pending`, bytes moving | Uploading 45% | determinate bar | none per percent; one polite "Uploading report.pdf" at start |
| upload `completed`, job `queued` | Queued | indeterminate, elapsed time | polite "Queued" |
| `started` / `progress {stage}` | Converting: {stage} | indeterminate, elapsed time | polite on stage change only |
| `retrying` | Retrying (attempt {n}) | indeterminate | polite |
| `warning` | step unchanged, warning count shown | none | polite "1 warning" |
| `succeeded` | Ready | none | polite "report.docx is ready to download" |
| `failed` | Failed | none | assertive, with the error copy |
| `canceled` | Canceled | none | polite |
| `expired` | Deleted (expired) | none | polite |

- **MUST** Implement the table above, so no two labels describe the same moment differently.
- **MUST** Show elapsed time during Queued and Converting ("Converting: write · 0:16"). CloudConvert's 16 seconds of an unlabeled indeterminate bar is the anti-pattern.
- **MUST** Reconnect the event stream with `Last-Event-ID` after a drop and say so in the row ("Reconnecting…") instead of failing the job in the UI.
- **SHOULD** Show the plan's processing deadline when a job runs long: "Stops after 2 minutes on the anonymous plan." (`timeout_s`).
- **SHOULD** Offer "Cancel" during Queued and Converting.

### Download

- **MUST** Name each download by its file: visible "Download report.docx" or a visually hidden suffix, so the accessible name is unique.
- **MUST** Set the `download` attribute (or a `Content-Disposition: attachment` response) so the browser saves instead of navigating away from `/app`.
- **MUST** Show the output size and any `JobWarning` messages before download.
- **SHOULD** Treat `410` from `/v1/jobs/{id}/outputs` as "This file was already deleted" with a "Convert again" action, not as an error dump.

### Delete and retention

- **MUST** Show the expiry on every finished row as an absolute local time plus a relative time: "Deletes automatically at 19:45 (in 58 min)". The relative part updates at least every minute. *More explicit than CloudConvert*, which shows no expiry in the flow.
- **MUST** Provide a labelled "Delete now" button (not an unlabeled ✕) that calls `DELETE /v1/jobs/{id}` and then confirms in the row and in a polite live region: "Deleted from {server host}. The input and output files are gone." Keep the row as a short tombstone so focus has somewhere to land. *More explicit than CloudConvert*, whose ✕ deletes silently.
- **MUST** Offer "Delete after download": after the browser starts the download, call `DELETE` and confirm the same way. Say what it does: "Deletes the files from the server as soon as your download starts."
- **MUST** Distinguish "remove from this list" (local only) from "Delete now" (server) wherever both exist. CloudConvert uses one ✕ for both and explains it only in the privacy policy.
- **SHOULD** When a job reaches `expired`, update the row to "Deleted automatically at 19:45" without user action, so the tab never offers a dead download link.

### Errors and limits

Map `ErrorDetail.code` to plain copy with a next step. Show the server `message` only under "Technical details", and never show engine stack traces or file system paths in the main copy.

| Code / status | Copy | Action |
|---|---|---|
| `payload_too_large` (413), `limit_exceeded` on size | This file is {size}. The {plan} plan accepts files up to {limit}. | Sign in for larger files, or convert on this device |
| `limit_exceeded` (page, memory or time budget) | This document needs more than the {plan} plan allows ({detail}). | Sign in, or try a smaller file |
| `quota_exceeded` (429) | You have used today's {n} cloud conversions. They reset at {time}. | Convert on this device; sign in |
| `rate_limited` (429) | Too many requests. Try again in {seconds} s. | Retry button enabled after the wait |
| `turnstile_required`, `turnstile_failed` | We could not confirm you are not a bot. | Show the challenge again |
| `unsupported_route` | {from} to {to} is not available on this server. | Pick another target |
| `unauthorized` (401) | Your session has ended. | Sign in again; the staged file stays |
| `unavailable` (503), network error | The server cannot be reached. Your file has not been uploaded. / Your file is safe on the server until {expiry}. | Retry |
| job `failed` | The conversion failed: {short reason}. | Retry, Delete now, Technical details |

- **MUST** Implement the mapping above and keep the staged file after any pre-upload error, so the user can switch to on-device conversion without picking it again.
- **MUST** Announce failures through `role="alert"` (or an assertive region) and show a failure icon in the summary line. CloudConvert's footer announces "1 file ready to convert" with a check mark after a failure; that is the anti-pattern.
- **MUST** Say whether the file is on the server after a failure: "The uploaded file is kept until 19:45 so you can retry. Delete now." *More explicit than CloudConvert*.
- **MUST** Make retry an explicit "Retry" button, not a re-enabled primary button.
- **MUST** Show the anonymous quota before the first upload ("20 cloud conversions a day, files up to 10 MB, without an account") from plan data, and refuse quota-exhausted uploads before transfer when the server says so.
- **SHOULD** Use `aria-disabled="true"` plus an explanation instead of `disabled` on the primary button, so keyboard users can reach it and hear why it does nothing ("Choose a target format first").

### Keyboard and ARIA

- **MUST** Every step works with the keyboard alone: choose file, choose destination, choose target, upload, cancel, download, delete, retry. The Playwright suite should cover this path.
- **MUST** Manage focus: after "Upload and convert", focus the row's status; after "Delete now", focus the tombstone message; after an error, focus the error summary or its first action. Never drop focus to `<body>`.
- **MUST** Use one polite live region for progress summaries and one assertive region for failures, and announce only step changes, not each percent.
- **MUST** Give every row control a unique accessible name that includes the file name: "Delete report.pdf from the server now", "Download report.docx", "Target format for report.pdf".
- **MUST** Keep visible focus indicators on every control, at least 2 px and 3:1 contrast, including header links.
- **MUST** Make the destination choice a native radio group (`fieldset` with `legend` "Where to convert") with "On this device" and "On {server host}".
- **SHOULD** Add a skip link to the converter on `/app`.
- **SHOULD** Keep touch targets at least 44×44 px on mobile, and place "Delete now" next to the result it deletes, not in the card corner.
- **SHOULD** Keep the format picker's good behaviour: dialog semantics, focus into search on open, Escape closes and returns focus to the trigger.

## Hand-off notes

- The cloud-mode worker owns the destination radio, the disclosure, staging, the Uppy presigned `PUT`, SSE progress, download, "Delete now", "Delete after download", the Turnstile step and the error mapping above.
- The jobs page worker should reuse the same step labels, expiry line, "Delete now" confirmation and error copy, so a job reads the same in `/app` and on `/jobs`.
- The docs worker should publish the privacy statement that the disclosure links to, with the same retention wording (one hour by default, deleted sooner on request, `t/` prefix backstop of one day for orphaned objects, file names never logged), and the anonymous limits from `docs/server.md`.
- The anonymous plan's `jobs_per_day` (20) and `max_input_bytes` (10 MiB) in `docs/server.md` are the numbers to show; read them from the server at run time rather than copying them into the UI.

## Unresolved questions

- The API exposes plan limits only through refusals. A small read endpoint (or fields on `/v1/capabilities`) for the caller's plan limits, remaining quota and retention would let the UI show limits before upload as required above. This is an API change outside this task.
- `/v1` has no client call to delete an upload that never became a job: `DELETE` exists only on `/v1/jobs/{id}`, and `api/uploads.rs` deletes an upload only when its completed file mismatches. Until an upload delete exists, "Cancel upload" must say "Upload canceled. Any partial data is deleted automatically at {expiry}." instead of "Nothing was kept". Adding the endpoint is an API change outside this task.
