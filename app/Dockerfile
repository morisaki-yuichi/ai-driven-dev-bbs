# syntax=docker/dockerfile:1

# rust-toolchain.tomlと同じバージョンで固定する(G: 再現性)。
FROM rust:1.95.0-bookworm AS chef
WORKDIR /app
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# 依存クレートだけを先にビルドし、レイヤキャッシュで以降のコールドビルドを避ける
# (foundation-plan.md §6「B緩和策」)。
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
# sqlx::query_as! はコンパイル時にDB接続を要求するが、評価者はclone直後(H-13)に
# DBなしでビルドする。.sqlx/のオフラインキャッシュを使う(foundation-plan.md §4.2)。
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin bbs

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/bbs /usr/local/bin/bbs
COPY static ./static
EXPOSE 3000
CMD ["/usr/local/bin/bbs"]
