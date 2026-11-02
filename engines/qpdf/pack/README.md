# qpdf pack development

The pack is built from qpdf 12.4.2, libjpeg-turbo 3.2.1 and zlib 1.3.2 on
every target. The upstream macOS binaries are not used. Linux arm64, both musl
targets and macOS require CI verification. `just pack-qpdf <qpdf_dir>` creates
an unsigned host pack; the upstream Linux development cache is permitted only
for local use and its additional dependencies are marked as unreviewed.

`licence-review.toml` records the approved, narrowly scoped libjpeg-turbo 3.2.1
exception.
`check-release.sh` fails closed while the owner's approval or the GNU static
runtime review is pending; the workflow also requires all six target jobs to
succeed before signing.
The declared `ariad-engine/2` entrypoint requires the host qpdf adapter, which
is being implemented separately.
