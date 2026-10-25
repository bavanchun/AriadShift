# syntax=docker/dockerfile:1
#
# AriadShift web app: the static export of apps/web (docs/web.md, "Static
# hosting") served by unprivileged nginx on port 3000. The API and auth base
# URLs, the Turnstile site key and the operator name are compiled into the
# site, so rebuild the image after changing them.
#
# From the repository root:
#   docker build -f infra/docker/web.Dockerfile -t ariadshift-web .
#
# The export is the same on every architecture, so the build stage runs on the
# build host's platform and only the nginx stage is per platform. The server
# runs as UID 101, needs no capabilities, and writes only to /tmp, so the
# container can run with a read-only root filesystem and a /tmp tmpfs.

ARG NODE=node:24.21.0-trixie-slim@sha256:173f125896c3b47ddf056734c7ea789d04595a6a08769a8f78e0df642781fb66
ARG NGINX=nginxinc/nginx-unprivileged:1.30.6-alpine-slim@sha256:5d8bb9437d74a600ec5dbf90c035451c724dc33eac8d3a71078f6f8612c345f6

FROM --platform=$BUILDPLATFORM ${NODE} AS build
ARG RUSTUP_VERSION=1.29.2
ARG PNPM_VERSION=12.9.1
# Pass --build-arg CARGO_BUILD_JOBS=N to bound compile parallelism.
ARG CARGO_BUILD_JOBS=3
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*
# rustup-init and pnpm are checked against their published SHA-256. The Rust
# toolchain and its wasm32 target come from rust-toolchain.toml.
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
RUN set -eu; \
    case "$(uname -m)" in \
        x86_64) asset=pnpm-linux-x64.tar.gz; sum=b4f58449ac02d9d24023ac89e17e05e8331f8ddc12ca1f489cf231bc2bb2bce1 ;; \
        aarch64) asset=pnpm-linux-arm64.tar.gz; sum=468294b77633d29889c6e4a79cefb1277eba30cd62e7d6a754218cb85a1d730d ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --retry 3 -o "/tmp/${asset}" "https://github.com/pnpm/pnpm/releases/download/v${PNPM_VERSION}/${asset}"; \
    echo "${sum}  /tmp/${asset}" | sha256sum -c -; \
    mkdir -p /opt/pnpm; \
    tar -xzf "/tmp/${asset}" -C /opt/pnpm; \
    ln -s /opt/pnpm/pnpm /usr/local/bin/pnpm; \
    rm "/tmp/${asset}"
ENV PATH=/root/.cargo/bin:${PATH} \
    NEXT_TELEMETRY_DISABLED=1
WORKDIR /src
COPY . .
# Same steps as `just wasm-build`: the browser module goes to packages/ariad-wasm/pkg.
RUN sh scripts/install-wasm-bindgen.sh
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked -p ariad-wasm --target wasm32-unknown-unknown --profile wasm-release \
    && .tools/bin/wasm-bindgen --target web --out-dir packages/ariad-wasm/pkg \
        target/wasm32-unknown-unknown/wasm-release/ariad_wasm.wasm
RUN pnpm install --frozen-lockfile --filter "@ariadshift/web..."
# Public base URLs for cloud mode; the defaults match infra/compose. The
# Turnstile site key and the operator name are optional; empty leaves them off.
ARG NEXT_PUBLIC_ARIAD_API_URL=http://localhost:8080
ARG NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3001
ARG NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=
ARG NEXT_PUBLIC_ARIAD_OPERATOR=
# Object-store origins of the API's presigned URLs, for the converter's CSP
# connect-src (space-separated); the default matches ARIAD_S3_PUBLIC_ENDPOINT
# in infra/compose.
ARG ARIADSHIFT_WEB_STORAGE_ORIGINS=http://localhost:8333
ENV NEXT_PUBLIC_ARIAD_API_URL=${NEXT_PUBLIC_ARIAD_API_URL} \
    NEXT_PUBLIC_ARIAD_AUTH_URL=${NEXT_PUBLIC_ARIAD_AUTH_URL} \
    NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=${NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY} \
    NEXT_PUBLIC_ARIAD_OPERATOR=${NEXT_PUBLIC_ARIAD_OPERATOR} \
    ARIADSHIFT_WEB_STORAGE_ORIGINS=${ARIADSHIFT_WEB_STORAGE_ORIGINS}
RUN ARIADSHIFT_WEB_NGINX_CONF=/src/web-nginx.conf pnpm --filter @ariadshift/web export

FROM ${NGINX} AS web
COPY LICENSE NOTICE THIRD_PARTY_LICENSES /usr/share/doc/ariadshift/
COPY --from=build /src/apps/web/out /srv/web
# The server and its headers are generated from apps/web/route-headers.mjs at
# export time: a CSP per page that pins its inline scripts, cross-origin
# isolation on /app only, and worker policies on the script chunks.
COPY --from=build /src/web-nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["wget", "-q", "--spider", "http://127.0.0.1:3000/"]
# Start nginx directly: the base image's entrypoint scripts rewrite files under
# /etc/nginx, which a read-only root filesystem refuses.
ENTRYPOINT ["nginx", "-g", "daemon off;"]
CMD []
