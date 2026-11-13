# ORYN Playwright runner (oryn.browser/v1).
# The base image tag MUST match the `playwright` version in package.json so the
# bundled browsers match what the library expects.

# ---- build ---------------------------------------------------------------
FROM mcr.microsoft.com/playwright:v1.65.0-noble AS build
WORKDIR /app
COPY package.json package-lock.json tsconfig.json tsconfig.build.json ./
RUN npm ci --no-audit --no-fund
COPY src ./src
RUN npm run build && npm prune --omit=dev

# ---- runtime -------------------------------------------------------------
FROM mcr.microsoft.com/playwright:v1.65.0-noble
ENV NODE_ENV=production \
    PORT=7400 \
    HOST=0.0.0.0 \
    MAX_CONCURRENCY=2 \
    ORYN_RUN_TIMEOUT_MS=300000
WORKDIR /app
COPY --from=build --chown=pwuser:pwuser /app/package.json ./package.json
COPY --from=build --chown=pwuser:pwuser /app/node_modules ./node_modules
COPY --from=build --chown=pwuser:pwuser /app/dist ./dist
USER pwuser
EXPOSE 7400
HEALTHCHECK --interval=15s --timeout=5s --start-period=20s --retries=3 \
  CMD node -e "fetch('http://127.0.0.1:'+(process.env.PORT||7400)+'/v1/health').then(r=>process.exit(r.ok?0:1),()=>process.exit(1))"
# Chromium needs a larger /dev/shm than Docker's default; run with --ipc=host or --shm-size=1g.
CMD ["node", "dist/server.js"]
