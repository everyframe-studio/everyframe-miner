# Supply reviewed digest-pinned images; never build an admitted release from tags.
ARG RUST_IMAGE
ARG RUNTIME_IMAGE
FROM ${RUST_IMAGE} AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY config ./config
RUN cargo build --release --locked --bins

FROM ${RUNTIME_IMAGE} AS cli
COPY --from=build /build/target/release/everycli /usr/local/bin/everycli
USER 1000:0
ENTRYPOINT ["/usr/local/bin/everycli"]

FROM ${RUNTIME_IMAGE} AS worker
COPY --from=build /build/target/release/everyframe-worker /usr/local/bin/everyframe-worker
USER 1000:0
ENTRYPOINT ["/usr/local/bin/everyframe-worker"]
