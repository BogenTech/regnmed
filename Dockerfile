# Runtime-only image: binaries are cross-compiled on the host by
# scripts/build-images.sh (aarch64-unknown-linux-musl, static) and copied
# in — no compilation in Docker. Contains both the API server and the
# regnmed CLI (migrate / verify-ledger / demo), so migration Jobs and
# debugging use the same image.
FROM gcr.io/distroless/static-debian12:nonroot
WORKDIR /app
COPY target/aarch64-unknown-linux-musl/release/regnmed-api /app/regnmed-api
COPY target/aarch64-unknown-linux-musl/release/regnmed /app/regnmed
ENTRYPOINT ["/app/regnmed-api"]
