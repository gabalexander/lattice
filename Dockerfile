# lattice in a container: lattice, git and Claude Code, and nothing else.
# docs/docker.md says how to run it, log Claude Code in once, and reach
# private repositories.
#
#   docker compose up -d
#   docker compose exec lattice claude auth login
#
# The web app and lattice are built in stages of their own; Claude Code's
# native build and mermaid are downloaded in another, mermaid checked
# against the SHA-256 lattice checks it against. The image keeps only what
# runs: Alpine, git, ssh and ripgrep, which Claude Code's Grep runs on
# musl, and those three files.

ARG ALPINE=3.22

FROM node:22-alpine AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-alpine AS lattice
# rusqlite builds SQLite from its source.
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY assets/ assets/
COPY src/ src/
COPY --from=web /src/web/build web/build
RUN cargo build --release --locked && strip target/release/lattice

FROM alpine:${ALPINE} AS downloads
RUN apk add --no-cache bash ca-certificates curl libgcc libstdc++ ripgrep
# `stable`, `latest`, or a version like 2.1.295: the image never updates
# it itself, so a newer one comes with an image built again.
ARG CLAUDE_CODE=stable
RUN curl -fsSL https://claude.ai/install.sh | bash -s "$CLAUDE_CODE" \
    && cp -L /root/.local/bin/claude /claude
RUN curl -fsSL -o /mermaid.min.js https://cdn.jsdelivr.net/npm/mermaid@11.17.2/dist/mermaid.min.js \
    && echo "581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8  /mermaid.min.js" \
    | sha256sum -c -

FROM alpine:${ALPINE}
RUN apk add --no-cache ca-certificates git libgcc libstdc++ openssh-client ripgrep \
    && adduser -D -u 1000 -s /bin/sh lattice \
    && mkdir -p /data /home/lattice/.claude /opt/lattice/cache/lattice/downloads/11.17.2 \
    && chown -R lattice:lattice /data /home/lattice/.claude /opt/lattice/cache
COPY --from=downloads /claude /usr/local/bin/claude
COPY --from=downloads /mermaid.min.js /opt/lattice/cache/lattice/downloads/11.17.2/mermaid.min.js
COPY --from=lattice /src/target/release/lattice /usr/local/bin/lattice
COPY docker/entrypoint.sh /usr/local/bin/lattice-entrypoint
COPY docker/git-credential-env /usr/local/bin/git-credential-env
# git asks the helper for a token, and reads the repositories mounted from
# the host as they are, whoever owns them there.
RUN git config --system credential.helper env \
    && git config --system --add safe.directory '*'
# lattice's settings and data in /data; Claude Code's login and settings in
# its own volume; the cache, mermaid in it, in the image. On musl, Claude
# Code's Grep runs the system's ripgrep.
ENV XDG_DATA_HOME=/data \
    XDG_CONFIG_HOME=/data/config \
    XDG_CACHE_HOME=/opt/lattice/cache \
    CLAUDE_CONFIG_DIR=/home/lattice/.claude \
    DISABLE_AUTOUPDATER=1 \
    USE_BUILTIN_RIPGREP=0
USER lattice
WORKDIR /home/lattice
VOLUME ["/data", "/home/lattice/.claude"]
EXPOSE 7347
ENTRYPOINT ["lattice-entrypoint"]
CMD ["lattice", "serve", "--listen", "0.0.0.0", "--port", "7347"]
