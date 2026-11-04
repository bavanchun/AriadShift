# Scripts

- `measure-pptx-fidelity.py`: Measures multiset word recall between PDF -> PPTX and PDF -> DOCX conversions across PDF fixtures using `pdftotext`, `pdfinfo`, and LibreOffice. The CLI binary path can be configured via the `ASHIFT_BIN` environment variable or passed as the first command-line argument:
  ```sh
  ASHIFT_BIN=/path/to/ashift python3 scripts/measure-pptx-fidelity.py
  # or
  python3 scripts/measure-pptx-fidelity.py /path/to/ashift
  ```
