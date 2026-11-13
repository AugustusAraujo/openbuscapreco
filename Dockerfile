FROM rust:1.89-slim AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/buscapreco /usr/local/bin/buscapreco
VOLUME ["/data"]
WORKDIR /data
EXPOSE 6500
ENV RUST_LOG=info
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
  CMD buscapreco healthcheck 6500 || exit 1
ENTRYPOINT ["buscapreco"]
CMD ["serve", "-c", "/data/config.toml"]
