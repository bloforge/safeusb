FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
COPY --from=build /src/target/release/safeusb /usr/local/bin/safeusb
ENTRYPOINT ["safeusb"]
CMD ["--help"]
