# Compression measurement

The Stirling-PDF 3.2.2 baseline is recorded in `crates/ariad-pdf/tests/data/stirling-level6.tsv`, including the image digest and date. The API used level 6 with grayscale, linearization, and normalization disabled. The local qpdf 12.4.2 reference used `--object-streams=generate --recompress-flate --compression-level=9`. These are reference sizes only, not an acceptance result.

| Input | Input bytes | Stirling bytes | qpdf bytes |
| --- | ---: | ---: | ---: |
| arxiv-multilingual-open-text-2201-05609-v2.pdf | 405642 | 136838 | 349808 |
| arxiv-tableformer-2203-01017-v1.pdf | 745181 | 206587 | 670876 |
| pd-vi-decree-39-2022.pdf | 2094488 | 2005188 | 2072649 |
| pd-vi-ip-law-155-2025.pdf | 1836969 | 1329700 | 1746420 |
| pd-vi-qd-1922-2026.pdf | 285024 | 230264 | 275853 |
| pd-en-gao-08-35-highlights.pdf | 67775 | 40911 | 53492 |
| vi-garden-image-only.pdf | 201163 | 54116 | 201177 |
| en-map-scan.pdf | 467832 | 148454 | 467886 |
| vi-garden-scan.pdf | 446222 | 137490 | 446276 |
| scanned-skewed-vi.pdf | 292305 | 83801 | 292175 |

qpdf returned status 3 for the IP law input, reporting three stream warnings; the other runs returned status 0. No output from qpdf or Stirling is committed. AriadShift output sizes, SSIM, elapsed time, target-size results, and the JPEG encoder comparison have not been measured.
