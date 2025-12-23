ARG TARGETARCH
FROM --platform=linux/${TARGETARCH} rustlang/rust:nightly-trixie-slim AS builder

ARG TARGETARCH

WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends gcc nodejs npm musl-tools && rm -rf /var/lib/apt/lists/* && case "$TARGETARCH" in amd64) rustup target add x86_64-unknown-linux-musl ;; arm64) rustup target add aarch64-unknown-linux-musl ;; *) echo "Unsupported architecture for rustup: $TARGETARCH" && exit 1 ;; esac

COPY . .
RUN case "$TARGETARCH" in amd64) TARGET_TRIPLE="x86_64-unknown-linux-musl"; TARGET_CPU="x86-64-v2" ;; arm64) TARGET_TRIPLE="aarch64-unknown-linux-musl"; TARGET_CPU="generic" ;; *) echo "Unsupported architecture: $TARGETARCH" && exit 1 ;; esac && RUSTFLAGS="-C link-arg=-s -C target-feature=+crt-static -C target-cpu=$TARGET_CPU -A unused" cargo build --bin cursor-api --release --target=$TARGET_TRIPLE && mkdir /app && cp target/$TARGET_TRIPLE/release/cursor-api /app/

# 运行阶段
FROM scratch

COPY --chown=1001:1001 --chmod=0700 --from=builder /app /app

WORKDIR /app

ENV PORT=3000
EXPOSE ${PORT}

USER 1001

ENTRYPOINT ["/app/cursor-api"]
