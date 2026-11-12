# syntax=docker/dockerfile:1
#
# Worker for the office pack: the server image plus LibreOffice 26.8 from The
# Document Foundation's release archives, checked against their published
# SHA-256. Create the server image first, then from the repository root:
#   docker build -f infra/docker/worker-office.Dockerfile \
#     --build-arg SERVER_IMAGE=ariadshift-server -t ariadshift-worker-office .
# The default command reads the office queue; see docs/server.md for sandbox
# policy and tmpfs/container memory sizing.

ARG UBUNTU=ubuntu:26.04@sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944
ARG SERVER_IMAGE=ariadshift-server

FROM ${UBUNTU} AS libreoffice
ARG LIBREOFFICE_VERSION=26.8.2
ARG LIBREOFFICE_ARCHIVE_VERSION=26.8.2.2
ARG LIBREOFFICE_STABLE_URL=https://download.documentfoundation.org/libreoffice/stable
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    case "$(uname -m)" in \
        x86_64) dir=x86_64; arch=x86-64; sum=8290c8f46cb33d6df8f681cb796e0697461ea070f86ec55b63c94d7eb9d9a476 ;; \
        aarch64) dir=aarch64; arch=aarch64; sum=54a780b666fcda7510c2eebe85265b71dd209db280fc16882fca5589015ca505 ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    primary_url="${LIBREOFFICE_STABLE_URL}/${LIBREOFFICE_VERSION}/deb/${dir}/LibreOffice_${LIBREOFFICE_VERSION}_Linux_${arch}_deb.tar.gz"; \
    archive_url="https://downloadarchive.documentfoundation.org/libreoffice/old/${LIBREOFFICE_ARCHIVE_VERSION}/deb/${dir}/LibreOffice_${LIBREOFFICE_ARCHIVE_VERSION}_Linux_${arch}_deb.tar.gz"; \
    curl -fsSL --retry 3 -o /tmp/libreoffice.tar.gz "${primary_url}" \
        || curl -fsSL --retry 3 -o /tmp/libreoffice.tar.gz "${archive_url}"; \
    echo "${sum}  /tmp/libreoffice.tar.gz" | sha256sum -c -; \
    mkdir -p /debs; \
    tar -xzf /tmp/libreoffice.tar.gz -C /tmp; \
    find /tmp -path '*/DEBS/*.deb' -exec mv {} /debs/ \; ; \
    rm -f /tmp/libreoffice.tar.gz; \
    ls /debs/*.deb >/dev/null

FROM ${SERVER_IMAGE}
USER root
RUN --mount=type=bind,from=libreoffice,source=/debs,target=/debs \
    apt-get update \
    && apt-get install -y --no-install-recommends \
        /debs/*.deb \
        fontconfig fonts-dejavu-core fonts-liberation2 fonts-noto-core \
        libcairo2 libcups2t64 libdbus-1-3 libglib2.0-0t64 libnss3 libsm6 libx11-xcb1 libxinerama1 \
    && rm -rf /var/lib/apt/lists/* \
    && ln -s /opt/libreoffice*/program/soffice /usr/local/bin/soffice
ENV ASHIFT_SOFFICE=/usr/local/bin/soffice
USER 0
CMD ["worker", "--packs", "office"]
