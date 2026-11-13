# Northwind Supply demo application (zero runtime dependencies).
FROM node:22-alpine

ENV NODE_ENV=production \
    PORT=4000 \
    HOST=0.0.0.0 \
    DEMO_FLAKY=1

WORKDIR /app
COPY --chown=node:node package.json openapi.json ./
COPY --chown=node:node src ./src
COPY --chown=node:node public ./public

USER node
EXPOSE 4000
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
  CMD wget -qO- "http://127.0.0.1:${PORT}/api/health" >/dev/null || exit 1

CMD ["node", "src/server.js"]
