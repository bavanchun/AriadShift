# syntax=docker/dockerfile:1
#
# AriadShift API and core worker: ariad-server, the ashift engine binary and
# Pandoc on Ubuntu 26.04 LTS. LibreOffice and the docling pack live in the
# worker-office and worker-docling images, which start from this one.
#
# From the repository root:
#   docker build -f infra/docker/server.Dockerfile -t ariadshift-server .
# docs/server.md covers the api, worker and migrate commands, required
# environment, sandbox policy, and tmpfs/container memory sizing.

ARG UBUNTU=ubuntu:26.04@sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944

FROM ${UBUNTU} AS compile
ARG RUSTUP_VERSION=1.29.2
# Pass --build-arg CARGO_BUILD_JOBS=N to bound compile parallelism.
ARG CARGO_BUILD_JOBS=3
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*
# rustup-init is checked against its published SHA-256; the toolchain itself
# comes from rust-toolchain.toml.
RUN set -eu; \
    case "$(uname -m)" in \
        x86_64) target=x86_64-unknown-linux-gnu; sum=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4 ;; \
        aarch64) target=aarch64-unknown-linux-gnu; sum=a9c168dbea81a84671c6b7a5c1795cb0eb699020110d107ddfca427bd797e697 ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --retry 3 -o /tmp/rustup-init "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${target}/rustup-init"; \
    echo "${sum}  /tmp/rustup-init" | sha256sum -c -; \
    chmod +x /tmp/rustup-init; \
    /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none; \
    rm /tmp/rustup-init
ENV PATH=/root/.cargo/bin:${PATH}
WORKDIR /src
COPY . .
RUN sh scripts/install-pandoc.sh
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p ariad-server -p ariad-cli \
    && mkdir -p /artifacts \
    && cp target/release/ariad-server target/release/ashift /artifacts/

FROM ${UBUNTU} AS server
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin ariad
COPY --from=compile /artifacts/ariad-server /artifacts/ashift /usr/local/bin/
COPY --from=compile /src/.tools/pandoc/bin/pandoc /opt/pandoc/bin/pandoc
COPY LICENSE NOTICE THIRD_PARTY_LICENSES /usr/share/doc/ariadshift/
# Pandoc runs as a separate process; the image aggregates it and never links it.
RUN printf '%s\n' \
    'This image contains Pandoc 3.12 (https://pandoc.org), an unmodified binary' \
    'from https://github.com/jgm/pandoc/releases/tag/3.12, licensed under the GNU' \
    'General Public License, version 2 or later. The license text is in' \
    '/usr/share/common-licenses/GPL-2 and the source code is available at' \
    'https://github.com/jgm/pandoc/tree/3.12. AriadShift invokes it as an' \
    'external program and does not link it.' \
    > /usr/share/doc/ariadshift/PANDOC
ENV ARIAD_BIND=0.0.0.0:8080 \
    ARIAD_ENGINE_PROGRAM=/usr/local/bin/ashift \
    ASHIFT_PANDOC=/opt/pandoc/bin/pandoc
EXPOSE 8080
# The root supervisor's original environment is inaccessible to UID 10001.
# Drop application privileges before reading configuration; the worker also
# disables dumpability to protect its own credentials from same-user helpers.
# Do not move USER 10001 above this: tini would retain readable credentials.
USER 0
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/bin/setpriv", "--reuid=10001", "--regid=10001", "--clear-groups", "--no-new-privs", "/usr/local/bin/ariad-server"]
CMD ["api"]
