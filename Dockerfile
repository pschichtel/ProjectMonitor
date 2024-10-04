FROM alpine:3.20 as builder

RUN apk add --update --no-cache rustup openssl openssl-dev openssl-libs-static alpine-sdk
RUN echo 1 | rustup-init --no-modify-path

RUN mkdir /build

WORKDIR /build

COPY . .

ARG BUILD_HASH="unknown"

RUN source /root/.cargo/env \
 && cargo build --release

FROM scratch

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/target/release/project_monitor /usr/local/bin/project-monitor

ENTRYPOINT [ "/usr/local/bin/project-monitor" ]

