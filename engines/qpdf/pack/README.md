# qpdf pack development

This is an incomplete, unsigned Linux development pack implementation. The
source compilation job remains experimental; a local probe linked system zlib
and libm and did not satisfy the dependency check. The upstream Linux binary
contains additional libraries and is rejected by the development assembler.
macOS archives remain blocked pending review of their OpenSSL and
libjpeg-turbo 3.2.0 contents. No signing or publishing implementation exists.

`licence-review.toml` records the narrowly scoped libjpeg-turbo 3.2.1 review.
`check-release.sh` fails closed while the owner's approval is pending; the
workflow also refuses release until the pack and target checks are complete.
The declared `ariad-engine/2` entrypoint requires the host qpdf adapter, which
is being implemented separately.
