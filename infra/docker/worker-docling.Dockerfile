# syntax=docker/dockerfile:1
#
# Worker for the docling pack: the office image plus the system Tesseract and
# the signed docling engine pack, installed and verified by
# `ashift engines install`. Create the office image first, then from the
# repository root:
#   docker build -f infra/docker/worker-docling.Dockerfile \
#     --build-arg SERVER_IMAGE=ariadshift-worker-office -t ariadshift-worker-docling .
# The signed pack must be published before this build can finish. The default
# command reads the docling queue; see docs/server.md for runtime sizing.

ARG SERVER_IMAGE=ariadshift-worker-office

FROM ${SERVER_IMAGE}
USER root
RUN apt-get update \
    && apt-get install -y --no-install-recommends tesseract-ocr \
    && rm -rf /var/lib/apt/lists/*
# Packs install under ASHIFT_DATA_DIR; workers only read them.
ENV ASHIFT_DATA_DIR=/opt/ariadshift
RUN mkdir -p "${ASHIFT_DATA_DIR}" \
    && ashift engines install docling \
    && chmod -R a+rX,go-w "${ASHIFT_DATA_DIR}"
USER 10001
CMD ["worker", "--packs", "docling"]
