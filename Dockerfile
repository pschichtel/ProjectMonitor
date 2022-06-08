FROM alpine:3.16 as builder

RUN apk add --update --no-cache rustup openssl openssl-dev openssl-libs-static alpine-sdk
RUN echo 1 | rustup-init --no-modify-path

RUN export PATH=$PATH:/root/.cargo/bin \
 && cargo install cargo-chef

RUN mkdir /build

WORKDIR /build

COPY . .

RUN source /root/.cargo/env \
 && cargo build --release

FROM scratch

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/target/release/project_monitor /usr/local/bin/project-monitor

ENTRYPOINT [ "/usr/local/bin/project-monitor" ]

