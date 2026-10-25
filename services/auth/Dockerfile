# syntax=docker/dockerfile:1
#
# AriadShift auth service: Better Auth on Hono and Node.js (services/auth).
# It applies its migrations to the `auth` schema before it listens, serves
# /api/auth/* on PORT (default 3000), and exposes the JWKS that ariad-server
# verifies tokens against.
#
# From the repository root:
#   docker build -f services/auth/Dockerfile -t ariadshift-auth .
#
# The runtime runs as the image's `node` user (UID 1000), needs no
# capabilities and writes nothing, so it runs with a read-only root filesystem.

# Same Node.js LTS image and pnpm release as infra/docker/web.Dockerfile.
ARG NODE=node:24.21.0-trixie-slim@sha256:173f125896c3b47ddf056734c7ea789d04595a6a08769a8f78e0df642781fb66

FROM ${NODE} AS deploy
ARG PNPM_VERSION=12.9.1
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
# The pnpm binary is checked against its published SHA-256.
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
ENV CI=true
WORKDIR /src
# Workspace manifests, the lockfile and the service sources only
# (services/auth/Dockerfile.dockerignore).
COPY . .
# The service runs its TypeScript sources directly (Node.js type stripping),
# so there is no build step: deploy copies src/ and installs production
# dependencies from the workspace lockfile.
RUN --mount=type=cache,id=pnpm-store,target=/root/.local/share/pnpm/store \
    pnpm --filter @ariadshift/auth --prod deploy /out

FROM ${NODE} AS auth
COPY LICENSE NOTICE THIRD_PARTY_LICENSES /usr/share/doc/ariadshift/
WORKDIR /app
COPY --from=deploy --chown=root:root /out /app
ENV NODE_ENV=production \
    PORT=3000
USER node
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["node", "-e", "fetch('http://127.0.0.1:' + (process.env.PORT || 3000) + '/api/auth/ok').then((r) => process.exit(r.ok ? 0 : 1), () => process.exit(1))"]
ENTRYPOINT ["node", "src/main.ts"]
CMD ["serve"]
