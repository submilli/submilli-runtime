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
# denies everything else, so it is the entire build context.

# Distroless has no shell, so `RUN mkdir -p /var/lib/submilli && chown` is not
# available, and the server cannot create the directory itself: uid 65532 has no
# write permission on /var/lib. Copying a tracked scaffold from the context does
# not work either, because git does not track empty directories. Hence a
# throwaway stage whose only product is one owned, empty directory. Nothing from
# this image ships, which is why it is tag-pinned rather than digest-pinned.
FROM busybox:1.38-musl AS scaffold
RUN mkdir -p /state

# Pinned by digest, not by the floating `:nonroot` tag — the difference between a
# reproducible release and one that silently changes underneath a retag. Renovate
# keeps the pin current (see .github/dependabot.yml).
#
# `static` rather than `cc`: the binaries are static musl and load no glibc.
# Not `scratch`: sentry and rmcp pull rustls-native-certs -> openssl-probe, which
# reads /etc/ssl/certs at runtime, and jiff reads /usr/share/zoneinfo for named
# time zones. This base ships both, plus a mode-1777 /tmp the ephemeral VFS root
# defaults to, and an /etc/passwd entry for uid 65532.
FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab

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

# Relocates five of the six state directories via `default_data_root()`; the
# server keeps them under `$SUBMILLI_HOME/server/` — blueprints, sessions,
# vfs/sessions, secrets, packages — beside the CLI's own packages/ and secrets/.
# A volume from a release that kept them directly under $SUBMILLI_HOME is moved
# into that shape on the first boot, except packages/: it stays at the top level
# as the read-only fallback the server searches after its own store. The sixth,
# the ephemeral VFS root, is a separate setting that defaults to the system temp
# dir; left alone it grows the container's writable layer instead of the volume.
# Mount /tmp as a tmpfs (see compose.yaml) or set SUBMILLI_VFS_EPHEMERAL_DIR.
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
