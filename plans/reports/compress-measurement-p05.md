# Compression measurement

The Stirling-PDF 3.2.2 level-6 reference is in `crates/ariad-pdf/tests/data/stirling-level6.tsv`, including the image digest and date. Grayscale, linearization, and normalization were disabled. The qpdf 12.4.2 reference used `--object-streams=generate --recompress-flate --compression-level=9`; qpdf is a development-only reference.

The following run used ebook at 96 dpi and JPEG quality 60, the PDFium pack, and first/middle/last pages rendered at 96 dpi. Ratio is AriadShift output divided by input. Times are host elapsed milliseconds. Both SSIM columns compare against the input. Six files meet both the Stirling size and 0.98 SSIM thresholds; the required seven were not reached. The table predates the subsequent image-memory preflight checks and bounded writer wrapper. A later PDF corpus run with those checks and a fixed `SOURCE_DATE_EPOCH` processed every input without error; its output sizes differed by at most a few bytes and again reached 6/10.

| Input | Input bytes | Stirling bytes | Ours bytes | Ratio | Ours SSIM | Stirling SSIM | ms | qpdf bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| arxiv-multilingual-open-text-2201-05609-v2.pdf | 405642 | 136838 | 170741 | 0.4209 | 1.0000 | 0.9976 | 26963 | 349808 |
| arxiv-tableformer-2203-01017-v1.pdf | 745181 | 206587 | 455538 | 0.6113 | 0.9996 | 0.9986 | 17830 | 670876 |
| pd-vi-decree-39-2022.pdf | 2094488 | 2005188 | 2011397 | 0.9603 | 0.9999 | 0.9996 | 8586 | 2072649 |
| pd-vi-ip-law-155-2025.pdf | 1836969 | 1329700 | 1200662 | 0.6536 | 0.9914 | 0.9745 | 27120 | 1746420 |
| pd-vi-qd-1922-2026.pdf | 285024 | 230264 | 188836 | 0.6625 | 0.9999 | 0.9985 | 3517 | 275853 |
| pd-en-gao-08-35-highlights.pdf | 67775 | 40911 | 50759 | 0.7489 | 1.0000 | 0.9999 | 2018 | 53492 |
| vi-garden-image-only.pdf | 201163 | 54116 | 36496 | 0.1814 | 0.9976 | 0.9879 | 10185 | 201177 |
| en-map-scan.pdf | 467832 | 148454 | 42971 | 0.0919 | 0.9927 | 0.9943 | 5856 | 467886 |
| vi-garden-scan.pdf | 446222 | 137490 | 40271 | 0.0902 | 0.9929 | 0.9944 | 5767 | 446276 |
| scanned-skewed-vi.pdf | 292305 | 83801 | 20614 | 0.0705 | 0.9944 | 0.9944 | 5589 | 292175 |

For the 50% target runs, none of the ten outputs landed within 5% of the target. The closest selected output and its SSIM were: 189903/1.0000, 436180/0.9869, 2010374/0.9988, 1063411/0.7647, 175421/0.9912, 50759/1.0000, 81687/0.9979, 195891/0.9945, 182724/0.9945, and 128099/0.9949 bytes/SSIM, in the table's order. Targets above those sizes are undershoots; targets below them produce `target_not_reached`. The fourth target run emitted `similarity_below_floor`.

The decree input also contains an AcroForm signature field with a `ByteRange`; `pdfsig` reports one signature in the input and none in the Stirling output, and `pdfinfo` reports an AcroForm only in the input. The AriadShift run preserves the field and reports that rewriting invalidates the signature. This is a material difference when interpreting the 6,209-byte size gap; removing the signature to meet the baseline would discard document data. The measurement does not prove how much of that gap each structural change accounts for.

The decree includes many 1-bit CCITT streams, which the approved image policy skips. For example, qpdf reports 59,526 raw bytes for input object 7 and 59,470 for the corresponding Stirling object 47; Stirling altered that CCITT stream. Changing the decision to skip CCITT would require a separate decision. qpdf returned status 3 for the IP-law input, reporting three stream warnings; the other reference runs returned status 0.

At quality 55, the same 1240 × 1754 RGB pixels produced a 75,364-byte JPEG with `image` 0.25.10 and a 50,130-byte JPEG with the system `cjpeg`; this is a libjpeg comparison rather than a direct `jpeg-encoder` crate benchmark, and equivalent perceived quality was not established. The `image` encoder source specifies 1×1 sampling for each component. The `jpeg-encoder` dependency was not added, so its size and SSIM remain unmeasured. No Stirling or qpdf output PDF is committed.
