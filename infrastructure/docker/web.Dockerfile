# ORYN web UI (Next.js standalone output). The browser only ever talks to this origin;
# `/api/*` is proxied to ORYN_API_INTERNAL_URL, so the session cookie stays first-party.
# syntax=docker/dockerfile:1.7

FROM node:22-alpine AS deps
WORKDIR /app
RUN corepack enable
COPY apps/web/package.json apps/web/pnpm-lock.yaml apps/web/pnpm-workspace.yaml ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store pnpm install --frozen-lockfile

FROM node:22-alpine AS build
WORKDIR /app
RUN corepack enable
COPY --from=deps /app/node_modules ./node_modules
COPY apps/web ./
# Next.js resolves rewrite destinations at build time, so the API address the web server
# proxies to is a build argument (the in-cluster / compose service name).
ARG ORYN_API_INTERNAL_URL=http://api:8080
ENV ORYN_API_INTERNAL_URL=$ORYN_API_INTERNAL_URL NEXT_TELEMETRY_DISABLED=1
RUN pnpm build

FROM node:22-alpine
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
RUN addgroup -S oryn && adduser -S -G oryn oryn
COPY --from=build --chown=oryn:oryn /app/.next/standalone ./
COPY --from=build --chown=oryn:oryn /app/.next/static ./.next/static
COPY --from=build --chown=oryn:oryn /app/public ./public
USER oryn
EXPOSE 3000
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=3 CMD wget -qO- http://127.0.0.1:3000/login >/dev/null || exit 1
CMD ["node", "server.js"]
