# Compression measurement

The Stirling-PDF 3.2.2 level-6 reference is `crates/ariad-pdf/tests/data/stirling-level6.tsv`, identical to the supplied baseline. Grayscale, linearization, and normalization were disabled. The qpdf 12.4.2 reference used `--object-streams=generate --recompress-flate --compression-level=9`; qpdf is development-only.

The ebook preset uses 96 dpi and JPEG quality 60 with `oxiarc-jpeg` 0.4.2 and 4:2:0 chroma sampling. The host rendered first, middle, and last pages at 96 dpi using PDFium. Both SSIM columns compare with the input. A pass requires output no larger than Stirling and SSIM at least 0.98. The rerun after font subsetting passed six files; the required seven were not reached. Times are host elapsed milliseconds and the ratio is output divided by input.

| Input | Input bytes | Stirling bytes | Ours bytes | Size gap bytes | Ratio | Ours SSIM | Stirling SSIM | Pass | ms | qpdf bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| arxiv-multilingual-open-text-2201-05609-v2.pdf | 405642 | 136838 | 166344 | +29506 | 0.4101 | 1.0000 | 0.9976 | no | 24406 | 349808 |
| arxiv-tableformer-2203-01017-v1.pdf | 745181 | 206587 | 452180 | +245593 | 0.6068 | 0.9996 | 0.9986 | no | 17591 | 670876 |
| pd-vi-decree-39-2022.pdf | 2094488 | 2005188 | 2011177 | +5989 | 0.9602 | 0.9998 | 0.9996 | no | 8728 | 2072649 |
| pd-vi-ip-law-155-2025.pdf | 1836969 | 1329700 | 1169330 | -160370 | 0.6366 | 0.9915 | 0.9745 | yes | 26977 | 1746420 |
| pd-vi-qd-1922-2026.pdf | 285024 | 230264 | 178616 | -51648 | 0.6267 | 0.9999 | 0.9985 | yes | 3588 | 275853 |
| pd-en-gao-08-35-highlights.pdf | 67775 | 40911 | 50759 | +9848 | 0.7489 | 1.0000 | 0.9999 | no | 2163 | 53492 |
| vi-garden-image-only.pdf | 201163 | 54116 | 24822 | -29294 | 0.1234 | 0.9976 | 0.9879 | yes | 9943 | 201177 |
| en-map-scan.pdf | 467832 | 148454 | 43271 | -105183 | 0.0925 | 0.9927 | 0.9943 | yes | 5557 | 467886 |
| vi-garden-scan.pdf | 446222 | 137490 | 40616 | -96874 | 0.0910 | 0.9929 | 0.9944 | yes | 5775 | 446276 |
| scanned-skewed-vi.pdf | 292305 | 83801 | 20966 | -62835 | 0.0717 | 0.9944 | 0.9944 | yes | 5522 | 292175 |

For the 50% target runs, no output landed within 5% of the target. Gap is output minus target, so negative values are undershoots. The ladder selects the highest-quality evaluated rung at or below target, or the smallest evaluated result with `target_not_reached`; a miss within 5% is not itself a D9 failure. The IP-law target emitted `similarity_below_floor`. The eight-rung cap is already fully allocated, and the approved instruction for a 6/10 result is to stop without tuning presets.

| Input | Target bytes | Output bytes | Gap bytes | Gap % | SSIM |
| --- | ---: | ---: | ---: | ---: | ---: |
| arxiv-multilingual-open-text-2201-05609-v2.pdf | 202821 | 183043 | -19778 | -9.75% | 1.0000 |
| arxiv-tableformer-2203-01017-v1.pdf | 372590 | 435350 | +62760 | +16.84% | 0.9869 |
| pd-vi-decree-39-2022.pdf | 1047244 | 2010336 | +963092 | +91.96% | 0.9988 |
| pd-vi-ip-law-155-2025.pdf | 918484 | 1044947 | +126463 | +13.77% | 0.7649 |
| pd-vi-qd-1922-2026.pdf | 142512 | 166207 | +23695 | +16.63% | 0.9912 |
| pd-en-gao-08-35-highlights.pdf | 33887 | 50759 | +16872 | +49.79% | 1.0000 |
| vi-garden-image-only.pdf | 100581 | 56545 | -44036 | -43.78% | 0.9979 |
| en-map-scan.pdf | 233916 | 204064 | -29852 | -12.76% | 0.9945 |
| vi-garden-scan.pdf | 223111 | 190829 | -32282 | -14.47% | 0.9945 |
| scanned-skewed-vi.pdf | 146152 | 137004 | -9148 | -6.26% | 0.9951 |

The current-branch host rerun used the PDFium pack and local Stirling outputs;
it measured 6/10 passes, 0 errors and 0/10 targets within 5%. The host
corpus test failed its required 7/10 assertion. The qpdf size column is the
previous run, not remeasured.

The decree input also contains an AcroForm signature field with a `ByteRange`; `pdfsig` reports one signature in the input and none in the Stirling output, and `pdfinfo` reports an AcroForm only in the input. The AriadShift run preserves the field and reports that rewriting invalidates the signature. This is a material difference when interpreting the 5,989-byte size gap; removing the signature to meet the baseline would discard document data. The measurement does not prove how much of that gap each structural change accounts for.

The decree includes many 1-bit CCITT streams, which the approved image policy skips. For example, qpdf reports 59,526 raw bytes for input object 7 and 59,470 for the corresponding Stirling object 47; Stirling altered that CCITT stream. Changing the decision to skip CCITT would require a separate decision. qpdf returned status 3 for the IP-law input, reporting three stream warnings; the other reference runs returned status 0.

No Stirling or qpdf output PDF is committed. The new encoder is pure Rust, Apache-2.0, and is enabled only by `codecs`; the prior 4:4:4 encoder remains in use outside lossy optimization.

## Stream inventory investigation

Before these font changes, an ebook run produced a 452,618-byte TableFormer
output, three bytes larger than the earlier 452,615-byte run.
This single-file inventory is not a new corpus acceptance measurement. `qpdf --json` was
used to classify stream `/Length` values by their references and dictionaries.
Categories are mutually exclusive; page content takes precedence over other
stream types. Object-stream and cross-reference containers are counted as
serialization overhead, not as unreachable user content.

| Category | Streams | Encoded bytes |
| --- | ---: | ---: |
| Embedded font programs | 28 | 243,043 |
| Embedded images | 7 | 78,294 |
| Form XObjects | 11 | 56,687 |
| Page content | 11 | 40,151 |
| Other reachable streams, including colour profiles and maps | 15 | 9,722 |
| Object and cross-reference stream containers | 2 | 15,461 |
| Non-stream dictionaries, headers and other serialization | — | 9,260 |
| Total | — | 452,618 |

The embedded programs comprise 116,760 bytes in 12 Type 1 `/FontFile` streams,
118,666 bytes in 15 `/FontFile2` streams, and 7,617 bytes in one
`/FontFile3` CID CFF stream. The inventory contains no separate metadata stream. Indirect metadata
dictionaries are included in non-stream overhead. Font bytes dominate: the
input contains embedded simple Type 1 and TrueType fonts as well as CID fonts.
The existing `subsetter` accepts OpenType input, strips its `cmap`, and converts
CFF to CID-keyed output. Consequently its output cannot simply replace a
simple-font program: the PDF encoding, character-to-glyph mapping, and text
extraction must be preserved. Raw Type 1 programs are outside its input format.
The current structural pipeline already garbage-collects, deduplicates streams,
and re-flates eligible streams at maximum compression; this investigation did
not establish a new 10-file result.

## Final-round input inventory

The following is an inventory of the original inputs, not the compressed
outputs. Encoded stream lengths come from `qpdf --json`, resolving indirect
lengths and counting each font program once. Font types and encodings come
from `pdffonts`. The previous table remains the output measurement.

| Input | Embedded fonts and encoded program bytes | Other dominant encoded streams |
| --- | --- | --- |
| arxiv-multilingual | Type 1, Builtin and Custom: 79,590 bytes; unembedded Standard Type 1 has no program bytes | Flate images 214,107; other streams 52,984 |
| arxiv-tableformer | Type 1, Builtin and Custom: 116,760; CID TrueType Identity-H and simple TrueType MacRoman: 119,469; CID Type 0C Identity-H: 7,617; unembedded Standard Type 1 has no program bytes | Flate images 286,098; other streams 76,179; forms 58,909 |
| pd-vi-decree-39-2022 | CID TrueType Identity-H: 31,717 | CCITT images 1,968,480; Flate images 40,751; other streams 1,307 |
| pd-en-gao-08-35-highlights | Type 1C WinAnsi: 20,247 | Other streams 35,351 |

The decree's only font belongs to the signature appearance form. Its
31,717-byte compressed program contains 67,344 decoded bytes. CCITT image
streams account for most of the document. The approved policy leaves CCITT
untouched, and deleting the signature would discard user data. The existing
subsetter already targets identity CID TrueType; a new simple-font Differences
or raw Type 1C implementation would not address this input's font. The
remaining three files do contain the types listed above, but their input byte
counts alone do not establish that a safe one-round change closes their gaps.

The current implementation scans 45 used glyphs in the decree signature
appearance and successfully subsets its identity CID TrueType program. A
diagnostic invocation of `glyph_usage` and `subset_type0` returned
`Subset { glyphs: 45 }`; the existing optimizer already invokes this path.
No additional lossless optimization was identified and implemented in this
round. A fresh host corpus run reproduced all ten rows above: 6/10 met both
size and SSIM, with zero processing errors. The original 7/10 assertion
failed. The PM accepted 6/10 as a recorded deviation pending the text-heavy
parity follow-up; the original seven-file target remains unmet.
