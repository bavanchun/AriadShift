# Page furniture implementation

Implemented page numbers, headers and footers, Bates numbering, linked contents pages, backgrounds and inversion using the shared drawing layer. Page labels retain their prefix and numbering style; contents pages retain original page objects and shift existing labels. Headers preserve the original Unicode filename and use the explicit date, SOURCE_DATE_EPOCH or injected clock in that order. The contents layout is limited to 50 pages, and links resolve to the original destinations.

Pagination marks use the Artifact/Pagination Header and Footer properties. PDF 2.0 PageNum and Bates structure subtypes were not assumed to be valid marked-content properties. Background and inversion use AriadShift private markers, with no watermark layer. The shared path API gained an isolated Difference-blend rectangle for inversion, and outline export gained a strict variant for contents links.

Focused checks: six tool test binaries passed (excluding the separately executed large-outline case); the 2,000-entry contents test passed in 38.45 seconds under a 2 GiB scope. Additional checks and cross-surface integration are recorded in the integration report.
