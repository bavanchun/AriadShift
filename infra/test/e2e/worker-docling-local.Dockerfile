# syntax=docker/dockerfile:1
# Local verification uses the documented unpacked development-pack override.
ARG SERVER_IMAGE=ariadshift-worker-office
FROM ${SERVER_IMAGE}
USER root
RUN apt-get update \
    && apt-get install -y --no-install-recommends tesseract-ocr \
    && rm -rf /var/lib/apt/lists/*
COPY --from=docling-pack / /opt/ariadshift/test-docling/
RUN chmod -R a+rX,go-w /opt/ariadshift/test-docling
ENV ASHIFT_PACK_DOCLING=/opt/ariadshift/test-docling
CMD ["worker", "--packs", "docling"]
