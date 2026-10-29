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
# The hosted agent worker (docs/agent.md §8.3) runs as its own CronJob
# from this same image — one model turn can take minutes, and the API's
# memory budget is measured under request load (docs/frugality.md).
COPY target/${TARGET}/release/regnmed-agent /app/regnmed-agent
ENTRYPOINT ["/app/regnmed-api"]
