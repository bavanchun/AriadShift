# qpdf pack development

The pack is built from qpdf 12.4.2, libjpeg-turbo 3.2.1 and zlib 1.3.2 on
every target. The upstream macOS binaries are not used. Linux arm64, both musl
targets and macOS require CI verification. `just pack-qpdf <qpdf_dir>` creates
an unsigned host pack; the upstream Linux development cache is permitted only
for local use and its additional dependencies are marked as unreviewed.

`build-inputs.txt` pins the immutable upstream archive URLs and SHA-256 digests
for qpdf, libjpeg-turbo and zlib, verified at download and build time and
recorded in Linux input receipts.
`runtime-sources.txt` pins corresponding-source archives for the reviewed GNU
runtime objects; for rebuilt Alpine `aports` trees, it pins the uncompressed
`git archive` `.tar` SHA-256 digests, verified before `gzip -n -9` compression
and checked on decompressed `.tar.gz` payloads during release verification.
`licence-review.toml` records the approved, narrowly scoped libjpeg-turbo 3.2.1
exception.
`check-release.sh` fails closed while the owner's approval or the GNU static
runtime review is pending; the workflow also requires all six target jobs to
succeed before signing.
The declared `ariad-engine/2` entrypoint requires the host qpdf adapter, which
is being implemented separately.
