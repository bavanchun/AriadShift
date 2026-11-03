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

The engine tool protocol runs one request per process. PDF OCR normally launches PDFium once to render each selected page and Tesseract once to recognize it. With auto-rotation, PDF OCR renders each page again and runs Tesseract separately for orientation and recognition. The host retains at most one rendered page at a time.
