# Overlay and watermark release-note draft

These notes are a draft for a release after 0.3.0; they do not change the 0.3.0 release scope.

- Fifteen PDF tools add text and image watermarks, stamps, page numbers, headers and footers, Bates numbers, images, text, QR codes, backgrounds and colour adjustments, and detect or remove explicitly selected watermark candidates. See [the tool reference](../../docs/tools.md) for their contracts.
- An invalid or empty `SOURCE_DATE_EPOCH` fails every native tool, recipe and MCP tool call with `invalid SOURCE_DATE_EPOCH`. A valid epoch fixes the UTC clock and reproducible PDF dates.
- Watermark detection and removal share a work budget within a removal request. Near the decode limit, detection can succeed while removal fails with the standard limit error.
