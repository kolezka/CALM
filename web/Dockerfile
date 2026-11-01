# Builds the static site and serves it with nginx. Coolify builds this with the
# Dockerfile build pack; set Base Directory to /web so the build context is the
# site, not the repo root.
#
# Astro 7 needs Node 22.23.4 or newer. This pins the current line, 26, rather
# than the oldest supported one, matching the Node the site is developed on.
# The runtime stage only carries nginx and static files; Node never ships.
FROM node:26-alpine AS build

WORKDIR /app

# Copy manifests first so a source-only change reuses the install layer.
COPY package.json bun.lock ./
RUN npm install -g bun@1.4.2 && bun install --frozen-lockfile

COPY . .
RUN bun run build

FROM nginx:1.27-alpine AS runtime

COPY --from=build /app/dist /usr/share/nginx/html

# Coolify routes traffic to this port. The image's default config already
# serves the directory above and falls back to index.html.
EXPOSE 80
