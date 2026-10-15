# Runtime image for `submilli-server`.
#
# This does not compile anything. The release workflow's existing matrix already
# cross-compiles static musl binaries for both Linux targets via cargo-zigbuild,
# so the build here is a file copy: the job lays the artifacts out as
# `dist/amd64/` and `dist/arm64/`, and buildx fills `TARGETARCH` per platform.
# Building in-container would either duplicate the zig toolchain or force
# QEMU-emulated arm64 Rust compilation.
#
# To build locally you must populate `dist/<arch>/` yourself — `.dockerignore`
# denies everything outside `dist/`, so it is the entire build context.
#
# The default target is that release image. The `render` target, built by
# .github/workflows/deploy-render.yml, runs the same binaries on a Debian base
# with a shell, adds this repository's packages, and also needs them compiled
# into `dist/packages/`.

# Distroless has no shell, so `RUN mkdir -p /var/lib/submilli && chown` is not
# available, and the server cannot create the directory itself: uid 65532 has no
# write permission on /var/lib. Copying a tracked scaffold from the context does
# not work either, because git does not track empty directories. Hence a
# throwaway stage whose only product is one owned, empty directory. Nothing from
# this image ships, which is why it is tag-pinned rather than digest-pinned.
FROM busybox:1.38-musl AS scaffold
RUN mkdir -p /state

# Pinned by digest, not by the floating `:nonroot` tag — the difference between a
# reproducible release and one that silently changes underneath a retag.
# Dependabot keeps the pin current (see .github/dependabot.yml).
#
# `static` rather than `cc`: the binaries are static musl and load no glibc.
# Not `scratch`: sentry and rmcp pull rustls-native-certs -> openssl-probe, which
# reads /etc/ssl/certs at runtime, and jiff reads /usr/share/zoneinfo for named
# time zones. This base ships both, plus a mode-1777 /tmp and an /etc/passwd
# entry for uid 65532.
#
# The `render` stage does not build on this one but repeats its settings, so
# read the note there before changing them.
FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab AS runtime

ARG TARGETARCH
ARG VERSION=0.0.0-dev
ARG REVISION=unknown

LABEL org.opencontainers.image.title="submilli-server" \
      org.opencontainers.image.description="Submilli HTTP execution server" \
      org.opencontainers.image.source="https://github.com/submilli/submilli-runtime" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"

# Both binaries ship. In a shell-less image the CLI is the only diagnostic an
# operator has over `docker exec` (`submilli server secret put`, `submilli
# server status`), and it is how blueprints are managed from outside the API.
COPY dist/${TARGETARCH}/ /usr/local/bin/

# uid 65532 and /var/lib/submilli are part of this image's public contract, not
# incidental detail: SUB-374's chart sets `runAsUser: 65532` and a matching
# `fsGroup` so the PersistentVolume is writable. Changing either is a breaking
# image release.
COPY --from=scaffold --chown=65532:65532 /state /var/lib/submilli

# Relocates every state directory via `default_data_root()`; the server keeps
# them under `$SUBMILLI_HOME/server/` — blueprints, sessions, secrets, packages,
# the database, and volumes/ (named volumes, session files, and ephemeral
# scratch) — beside the CLI's own packages/ and secrets/. The top-level
# packages/ is the read-only fallback the server searches after its own store.
ENV SUBMILLI_HOME=/var/lib/submilli \
    HOST=0.0.0.0 \
    TZ=Etc/UTC \
    SSL_CERT_DIR=/etc/ssl/certs

# Deliberately no VOLUME directive. Declaring one means every `docker run`
# without an explicit `-v` silently creates an anonymous volume that survives
# `docker rm` and accumulates invisibly. The intended mount is documented in
# compose.yaml instead.

# 0.0.0.0 inside the container is required for the container network to reach the
# server; host exposure is controlled at publish time. The API requires a bearer
# token, but it speaks plain HTTP, so every documented example still publishes as
# `127.0.0.1:8128:8128` on a dedicated user-defined network: the token is one
# layer and reachability is the other.
EXPOSE 8128

USER 65532:65532

# The probe is a separate process and cannot inherit CMD flags, so it resolves
# the address from $SUBMILLI_BIND/$SUBMILLI_PORT or --config — the same ladder
# the server binds with. It calls /healthz, which needs no token and reveals
# nothing, so the probe works without any credential in its environment.
#
# Docker and Compose honor this; Kubernetes ignores a Dockerfile HEALTHCHECK, so
# the chart points its own probes at /healthz.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/submilli-server", "--health-check"]

# No init process. An init exists to reap zombies and forward signals to
# children; the server spawns no subprocesses and installs its own SIGTERM/SIGINT
# handlers, so the binary is a correct PID 1 on its own.
#
# No CMD, and a bare `docker run IMAGE` exits non-zero: the server refuses to
# start until it has an API token (`-e SUBMILLI_SERVER_TOKEN`, as compose.yaml
# does, or `api_tokens` in a config file $SUBMILLI_CONFIG points at), or the
# operator opts out with SUBMILLI_ALLOW_UNAUTHENTICATED=1. Baking either choice
# into the image would make it the default for everyone who pulls it.
ENTRYPOINT ["/usr/local/bin/submilli-server"]

# The image Render runs (see .github/workflows/deploy-render.yml): the binaries
# from `runtime` plus this repository's packages. Unlike the release image, it has a
# shell, so an operator can open one from Render's dashboard or over SSH. Render
# does not support shells in distroless images, so this stage starts from Debian
# slim rather than from `runtime`. The shell runs as uid 65532, which owns the
# server's state, so whoever can open it can read and change the database and
# secrets.
#
# Everything else repeats `runtime`, so keep the two in step: same uid, state
# directory, environment, port, health check and entrypoint.
FROM debian:12-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS render

ARG VERSION=0.0.0-dev
ARG REVISION=unknown

LABEL org.opencontainers.image.title="submilli-server-dev" \
      org.opencontainers.image.description="Submilli HTTP execution server, with a shell, for Render" \
      org.opencontainers.image.source="https://github.com/submilli/submilli-runtime" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"

# `ca-certificates` and `tzdata` stand in for the CA roots and zone data that
# distroless ships in `runtime`, needed for the reasons given above that stage.
# The user and the state directory stand in for distroless's `nonroot` entry and
# the `scaffold` stage.
#
# The rest is what Render needs to open a shell for a non-root user
# (https://render.com/docs/ssh): a login shell and a ~/.ssh with mode 0700. sshd
# also refuses an account whose password starts with `!`, which is how `useradd`
# leaves it, so the password is `*`: no password can log in, but SSH keys still
# can.
#
# A login shell over SSH does not get the image's ENV, so
# /etc/profile.d/submilli.sh sets SUBMILLI_HOME there, unless the session
# already has one. Without it the CLI would keep its own secrets and packages in
# ~/.submilli instead. A command run as `ssh <host> <command>` starts no login
# shell, so it still needs `bash -lc`.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 nonroot \
    && useradd --uid 65532 --gid 65532 --create-home --home-dir /home/nonroot \
        --shell /bin/bash --password '*' nonroot \
    && install -d -m 0700 -o 65532 -g 65532 /home/nonroot/.ssh \
    && install -d -o 65532 -g 65532 /var/lib/submilli \
    && echo 'export SUBMILLI_HOME="${SUBMILLI_HOME:-/var/lib/submilli}"' \
        > /etc/profile.d/submilli.sh

COPY --from=runtime /usr/local/bin/ /usr/local/bin/

# Compiled by the workflow with `submilli build publish-local`, into the CLI's
# store under $SUBMILLI_HOME. COPY leaves them owned by root, which is fine: the
# server (uid 65532) only reads that store, as its fallback. The fallback is
# always $SUBMILLI_HOME/packages, so a deployment that sets SUBMILLI_HOME
# elsewhere hides these; move state with the per-path settings instead.
COPY dist/packages/ /var/lib/submilli/packages/

ENV SUBMILLI_HOME=/var/lib/submilli \
    HOST=0.0.0.0 \
    TZ=Etc/UTC \
    SSL_CERT_DIR=/etc/ssl/certs

EXPOSE 8128

USER 65532:65532

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/submilli-server", "--health-check"]

ENTRYPOINT ["/usr/local/bin/submilli-server"]

# Last, so a build that names no target still produces the release image.
FROM runtime
