# syntax=docker/dockerfile:1
#
# Worker for the office pack: the server image plus LibreOffice 26.8 from The
# Document Foundation's release archives, checked against their published
# SHA-256. Create the server image first, then from the repository root:
#   docker build -f infra/docker/worker-office.Dockerfile \
#     --build-arg SERVER_IMAGE=ariadshift-server -t ariadshift-worker-office .
# Run with a tmpfs at /tmp; the default command reads the office queue.

ARG UBUNTU=ubuntu:26.04@sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944
ARG SERVER_IMAGE=ariadshift-server

FROM ${UBUNTU} AS libreoffice
ARG LIBREOFFICE_VERSION=26.8.1
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    case "$(uname -m)" in \
        x86_64) dir=x86_64; arch=x86-64; sum=30903df3b9f61360d9660cd707de48cd2831469114492a5008ed58a0ac77d044 ;; \
        aarch64) dir=aarch64; arch=aarch64; sum=1b069c20dd237f6decad3ea02cb02f39fdf458b03d09c2a9facb7b9b71e6a27a ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    archive="LibreOffice_${LIBREOFFICE_VERSION}_Linux_${arch}_deb.tar.gz"; \
    curl -fsSL --retry 3 -o "/tmp/${archive}" \
        "https://download.documentfoundation.org/libreoffice/stable/${LIBREOFFICE_VERSION}/deb/${dir}/${archive}"; \
    echo "${sum}  /tmp/${archive}" | sha256sum -c -; \
    mkdir -p /debs; \
    tar -xzf "/tmp/${archive}" -C /tmp; \
    find /tmp -path '*/DEBS/*.deb' -exec mv {} /debs/ \; ; \
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
USER 10001
CMD ["worker", "--packs", "office"]
