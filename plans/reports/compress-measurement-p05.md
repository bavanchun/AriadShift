# Compression measurement

The Stirling-PDF 3.2.2 level-6 reference is `crates/ariad-pdf/tests/data/stirling-level6.tsv`, identical to the supplied baseline. Grayscale, linearization, and normalization were disabled. The qpdf 12.4.2 reference used `--object-streams=generate --recompress-flate --compression-level=9`; qpdf is development-only.

The ebook preset uses 96 dpi and JPEG quality 60 with `oxiarc-jpeg` 0.4.2 and 4:2:0 chroma sampling. The host rendered first, middle, and last pages at 96 dpi using PDFium. Both SSIM columns compare with the input. A pass requires output no larger than Stirling and SSIM at least 0.98. Six files pass; the required seven were not reached. Times are host elapsed milliseconds and the ratio is output divided by input.

| Input | Input bytes | Stirling bytes | Ours bytes | Size gap bytes | Ratio | Ours SSIM | Stirling SSIM | Pass | ms | qpdf bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| arxiv-multilingual-open-text-2201-05609-v2.pdf | 405642 | 136838 | 166343 | +29505 | 0.4101 | 1.0000 | 0.9976 | no | 26227 | 349808 |
| arxiv-tableformer-2203-01017-v1.pdf | 745181 | 206587 | 452615 | +246028 | 0.6074 | 0.9996 | 0.9986 | no | 18876 | 670876 |
| pd-vi-decree-39-2022.pdf | 2094488 | 2005188 | 2011177 | +5989 | 0.9602 | 0.9998 | 0.9996 | no | 8627 | 2072649 |
| pd-vi-ip-law-155-2025.pdf | 1836969 | 1329700 | 1186193 | -143507 | 0.6457 | 0.9915 | 0.9745 | yes | 26888 | 1746420 |
| pd-vi-qd-1922-2026.pdf | 285024 | 230264 | 187530 | -42734 | 0.6579 | 0.9999 | 0.9985 | yes | 3514 | 275853 |
| pd-en-gao-08-35-highlights.pdf | 67775 | 40911 | 50758 | +9847 | 0.7489 | 1.0000 | 0.9999 | no | 2261 | 53492 |
| vi-garden-image-only.pdf | 201163 | 54116 | 24820 | -29296 | 0.1234 | 0.9976 | 0.9879 | yes | 11449 | 201177 |
| en-map-scan.pdf | 467832 | 148454 | 43270 | -105184 | 0.0925 | 0.9927 | 0.9943 | yes | 6417 | 467886 |
| vi-garden-scan.pdf | 446222 | 137490 | 40615 | -96875 | 0.0910 | 0.9929 | 0.9944 | yes | 6481 | 446276 |
| scanned-skewed-vi.pdf | 292305 | 83801 | 20965 | -62836 | 0.0717 | 0.9944 | 0.9944 | yes | 6211 | 292175 |

For the 50% target runs, no output landed within 5% of the target. Gap is output minus target, so negative values are undershoots. The ladder selects the highest-quality evaluated rung at or below target, or the smallest evaluated result with `target_not_reached`; a miss within 5% is not itself a D9 failure. The IP-law target emitted `similarity_below_floor`. The eight-rung cap is already fully allocated, and the approved instruction for a 6/10 result is to stop without tuning presets.

| Input | Target bytes | Output bytes | Gap bytes | Gap % | SSIM |
| --- | ---: | ---: | ---: | ---: | ---: |
| arxiv-multilingual-open-text-2201-05609-v2.pdf | 202821 | 183042 | -19779 | -9.75% | 1.0000 |
| arxiv-tableformer-2203-01017-v1.pdf | 372590 | 435786 | +63196 | +16.96% | 0.9869 |
| pd-vi-decree-39-2022.pdf | 1047244 | 2010335 | +963091 | +91.96% | 0.9988 |
| pd-vi-ip-law-155-2025.pdf | 918484 | 1061810 | +143326 | +15.60% | 0.7649 |
| pd-vi-qd-1922-2026.pdf | 142512 | 175117 | +32605 | +22.88% | 0.9912 |
| pd-en-gao-08-35-highlights.pdf | 33887 | 50758 | +16871 | +49.79% | 1.0000 |
| vi-garden-image-only.pdf | 100581 | 56545 | -44036 | -43.78% | 0.9979 |
| en-map-scan.pdf | 233916 | 204064 | -29852 | -12.76% | 0.9945 |
| vi-garden-scan.pdf | 223111 | 190829 | -32282 | -14.47% | 0.9945 |
| scanned-skewed-vi.pdf | 146152 | 137003 | -9149 | -6.26% | 0.9951 |

The decree input also contains an AcroForm signature field with a `ByteRange`; `pdfsig` reports one signature in the input and none in the Stirling output, and `pdfinfo` reports an AcroForm only in the input. The AriadShift run preserves the field and reports that rewriting invalidates the signature. This is a material difference when interpreting the 5,989-byte size gap; removing the signature to meet the baseline would discard document data. The measurement does not prove how much of that gap each structural change accounts for.

The decree includes many 1-bit CCITT streams, which the approved image policy skips. For example, qpdf reports 59,526 raw bytes for input object 7 and 59,470 for the corresponding Stirling object 47; Stirling altered that CCITT stream. Changing the decision to skip CCITT would require a separate decision. qpdf returned status 3 for the IP-law input, reporting three stream warnings; the other reference runs returned status 0.

No Stirling or qpdf output PDF is committed. The new encoder is pure Rust, Apache-2.0, and is enabled only by `codecs`; the prior 4:4:4 encoder remains in use outside lossy optimization.
