# Runtime-only image: binaries are cross-compiled on the host by
# scripts/build-images.sh (static musl) and copied in — no compilation in
# Docker. Contains both the API server and the regnmed CLI (migrate /
# verify-ledger / demo), so migration Jobs and debugging use the same image.
#
# TARGET has NO DEFAULT on purpose. The two clusters are different
# architectures — the colima dev cluster on the Mac is aarch64, the homelab
# nodes are x86_64 — and a default would let a bare `docker build .` produce
# an image for the wrong one. Wrong-architecture binaries in a distroless
# image fail at container start with `exec format error`, far from the cause.
# Unset, the COPY path has an empty segment and the build fails here instead.
ARG TARGET
FROM gcr.io/distroless/static-debian12:nonroot
ARG TARGET
WORKDIR /app
COPY target/${TARGET}/release/regnmed-api /app/regnmed-api
COPY target/${TARGET}/release/regnmed /app/regnmed
ENTRYPOINT ["/app/regnmed-api"]
