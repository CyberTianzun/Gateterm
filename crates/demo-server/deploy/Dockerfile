# Build from the repository root; no credentials are part of the image/context.
ARG BUILDER_IMAGE=rust:1.93.1-alpine3.23
FROM ${BUILDER_IMAGE} AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY clients/desktop-web ./clients/desktop-web
RUN cargo build --locked --release --target x86_64-unknown-linux-musl -p gateterm-demo

FROM scratch
COPY --from=build /src/target/x86_64-unknown-linux-musl/release/gateterm-demo /gateterm-demo
USER 65532:65532
ENV TOKIO_WORKER_THREADS=2
ENTRYPOINT ["/gateterm-demo"]
CMD ["--host", "127.0.0.1", "--port", "17685"]
