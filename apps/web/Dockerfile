# Build context is the repo root (see docker-compose.yml) because this is an
# npm-workspaces monorepo: apps/web has no lockfile of its own, and Next.js's
# standalone output traces files relative to the workspace root.

# ---- Dependencies -----------------------------------------------------------
FROM node:20-bookworm-slim AS deps
WORKDIR /repo
COPY package.json package-lock.json ./
COPY apps/web/package.json apps/web/package.json
COPY packages/shared/package.json packages/shared/package.json
RUN npm ci

# ---- Build --------------------------------------------------------------
FROM node:20-bookworm-slim AS builder
WORKDIR /repo
COPY --from=deps /repo/node_modules ./node_modules
COPY . .
ARG NEXT_PUBLIC_API_URL=http://localhost:8000
ENV NEXT_PUBLIC_API_URL=${NEXT_PUBLIC_API_URL}
RUN npm run build --workspace=apps/web

# ---- Runtime ------------------------------------------------------------
FROM node:20-bookworm-slim AS runner
WORKDIR /app
ENV NODE_ENV=production
ENV PORT=3000

# 'standalone' output mirrors the monorepo layout, so the entrypoint lives
# under apps/web/ inside the copied tree.
COPY --from=builder /repo/apps/web/.next/standalone ./
COPY --from=builder /repo/apps/web/.next/static ./apps/web/.next/static

EXPOSE 3000
CMD ["node", "apps/web/server.js"]
