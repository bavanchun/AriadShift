# Overlay and watermark implementation status

The 15 approved tools and their shared drawing primitives are implemented. The branch integrates `origin/dev` at `5e82c117`; the slice reports cover [drawing](worker-p06-s1.md), [marks](worker-p06-s2.md), [page furniture](worker-p06-s3.md), [colour](worker-p06-s4.md) and [watermark detection and removal](worker-p06-s5.md). The registry, schemas, inline removal report, documentation, notices and fuzz wiring are included.

Linux checks passed after the documented retries in [the integration report](worker-p06-integrator.md). The unmaintained-status advisory for the approved font parser has a documented exception for trusted, hash-pinned bundled fonts. Four core/PDF tests and 11 host/CLI tests were ignored for the reasons in that report and are not counted as passed.

The overlay font subsetter only selects bundled Libertinus faces; its boundary regression passes and [issue #205](https://github.com/bavanchun/AriadShift/issues/205) tracks replacing the parser. Typst's closed world selects only its seven bundled, hash-verified fonts. The independent Argon delta review returned MERGE and confirmed the eight findings are fixed and tested. The Mac PM handles Sonnet review. Manual macOS Preview, Chrome, Acrobat and phone QR checks are pending; CI results are separate.
