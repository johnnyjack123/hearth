# Server of llm-harness with the web app, for amd64 and arm64 (Raspberry Pi).
# Build from the repository root:
#   docker build -f apps/server/Dockerfile -t llm-harness-server .
# Settings come from environment variables, see .env.example.

FROM docker.io/library/node:24-trixie-slim AS build
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
WORKDIR /src

# Manifests first, so dependency layers survive source changes.
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml tsconfig.base.json ./
COPY apps/server/package.json apps/server/
COPY apps/web/package.json apps/web/
COPY packages/shared/package.json packages/shared/
RUN pnpm install --frozen-lockfile

COPY packages/shared packages/shared
COPY apps/server apps/server
COPY apps/web apps/web
RUN pnpm --filter @harness/web build \
 && pnpm --filter @harness/server build \
 && pnpm --filter @harness/server deploy --prod /out

FROM docker.io/library/node:24-trixie-slim
ENV NODE_ENV=production \
    HOST=0.0.0.0 \
    PORT=8080 \
    DATA_DIR=/data \
    WEB_DIST=/app/web
WORKDIR /app
COPY --from=build /out/package.json ./
COPY --from=build /out/node_modules ./node_modules
COPY --from=build /src/apps/server/dist ./dist
COPY --from=build /src/apps/web/dist ./web
# The database and the generated secret live in the volume; the image runs
# as the unprivileged `node` user of the base image.
RUN mkdir -p /data && chown node:node /data
# The workspace version (Cargo.toml) and commit, from the workflow; the web
# app and paired computers compare it with harness's (`src/version.ts`).
ARG HARNESS_VERSION=dev
ARG HARNESS_COMMIT=
ENV HARNESS_VERSION=${HARNESS_VERSION} \
    HARNESS_COMMIT=${HARNESS_COMMIT}
USER node
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
  CMD ["node", "-e", "fetch('http://127.0.0.1:8080/api/health').then(r => process.exit(r.ok ? 0 : 1), () => process.exit(1))"]
CMD ["node", "dist/index.js"]
