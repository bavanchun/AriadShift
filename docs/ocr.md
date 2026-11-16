# OCR and scan tools

OCR uses the `tesseract` engine pack; PDF page rendering, deskew and splitting also require the `pdfium` pack. Both engines run outside the host in a network-isolated sandbox. Pack installation and the explicit extra-language cache are described in [packs.md](packs.md#the-tesseract-pack). Windows and musl OCR are deferred ([#202](https://github.com/bavanchun/AriadShift/issues/202)).

`pdf.ocr` accepts a PDF and a page range. It defaults to all pages, languages `vie` and `eng`, 300 dpi, `skip-text` mode and a PDF output. `mode: "skip-text"` leaves pages with existing text alone. `force` adds a layer and warns if text already exists. `redo` removes an existing invisible text layer before recognition; it refuses removal when text or clipping cannot be safely separated. `deskew` and `auto_rotate` are opt-in. Deskew and orientation use page transforms and preserve the underlying image bytes. `clean` explicitly permits raster replacement of eligible image-only pages; pages with other content or annotations are preserved and produce a warning. Rewriting a signed PDF invalidates its signatures and produces the standard warning.

`output` is `pdf`, `pdf+txt`, or `pdf+hocr`. The latter two publish two artifacts atomically and require an output directory. For example:

```sh
mkdir -p results
ashift tool pdf.ocr scan.pdf -o results/ --param 'output=pdf+txt'
ashift tool pdf.ocr scan.pdf -o searchable.pdf --param 'languages=["vie","eng"]' --param 'mode=redo'
```

`image.ocr` accepts PNG, JPEG, WebP, BMP and single-frame TIFF, with `output` set to `pdf`, `txt`, or `hocr`. Its schema permits 1–200 images; the Free plan still permits at most 25 inputs and 250 MB per run. `pdf.auto_rotate` uses orientation confidence and leaves uncertain pages unchanged. `pdf.deskew` corrects skew with page transforms; `pdf.scan_split` finds scan regions and keeps the original page image streams; `pdf.scanner_effect` renders a deterministic scan-like appearance.

Progress and cancellation apply across the entire job. The OCR time budget is 600 seconds plus 15 seconds per selected page, capped by any explicit timeout; without one, it is capped at 14,400 seconds (four hours). Input, page, output, image, word, memory and time limits remain in force. OCR is not a guarantee of textual accuracy: check recognition and page orientation against the source, particularly on faint, complex or handwritten scans. See `ashift tools --json` for the complete parameter schemas and [tools.md](tools.md) for output naming and publication rules.

The engine tool protocol runs one request per process. PDF OCR normally launches PDFium once to render each selected page and Tesseract once to recognize it. With auto-rotation, PDF OCR renders each page again and runs Tesseract separately for orientation and recognition.

## Parallel pages

Pages are processed by a bounded pool of workers. Each worker handles one page at a time, so at most that many PDFium or Tesseract processes run at once. The default is the number of CPU cores, capped at 4, and never more than the number of selected pages. `ASHIFT_OCR_WORKERS` (an integer from 1 to 8) overrides it; any other value is ignored. Set it to `1` for strictly sequential processing on a small machine.

- **Same result as sequential.** Workers only render and recognize. Their results are applied to the document, the text and hOCR sidecars, the warnings and the progress events strictly in page order, so output bytes, warning order and the first reported error match a sequential run. Engine-internal progress events are not forwarded while pages run concurrently; the per-page `ocr` progress event is.
- **Bounded memory.** A page may start only while it is fewer than `workers` pages ahead of the next page to be applied, so the in-flight and waiting page results (recognized text, and the decoded cleaned raster when `clean` is on) never exceed `workers`. The host still never holds more than one decoded cleaned image per worker.
- **Deadline and cancellation.** All workers share the job deadline and cancellation: each engine call receives only the time left, and cancelling the job (or the first page failure) cancels in-flight engine processes and starts no further page.
- **Models staged once.** Extra language models are hash-verified and copied into the job workspace once per job, not once per page. Each additional worker's private workspace receives a plain copy of the verified files, and the Tesseract engine still verifies the model it loads inside its sandbox.

## Cleaned page images

With `clean`, PDFium writes the cleaned page as a PNG artifact and the host decodes it before replacing the page raster. The decode stays in the host because `scan.render` publishes a PNG artifact, and handing raw pixels back from the engine would change the `ariad-engine/2` artifact contract. The host calls the decoder the Tesseract engine uses, with the job limits and deadline: encoded size at most the smaller of `max_input_bytes` and 128 MB; width and height at most 16,384; at most 100 million pixels; allocation capped by the smaller of `max_memory_mb` (512 MB when unset) and `max_decompressed_bytes`; PNG, JPEG, WebP, BMP or single-frame TIFF only; and the job deadline checked between decode steps. A cleaned image that fails to decode or exceeds a bound fails the job with a limit error.
