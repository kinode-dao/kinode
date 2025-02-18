FROM --platform=$BUILDPLATFORM alpine AS downloader_start
ARG VERSION
ARG TARGETARCH
WORKDIR /tmp/download
RUN apk update && apk add unzip wget --no-cache

FROM downloader_start AS downloader_amd64
ADD "https://github.com/hyperware-ai/hyperdrive/releases/download/${VERSION}/hyperdrive-x86_64-unknown-linux-gnu.zip" hyperdrive-x86_64-unknown-linux-gnu.zip
RUN unzip hyperdrive-x86_64-unknown-linux-gnu.zip

FROM downloader_start AS downloader_arm64
ADD "https://github.com/hyperware-ai/hyperdrive/releases/download/${VERSION}/hyperdrive-aarch64-unknown-linux-gnu.zip" hyperdrive-aarch64-unknown-linux-gnu.zip
RUN unzip hyperdrive-aarch64-unknown-linux-gnu.zip

FROM downloader_${TARGETARCH} AS downloader

FROM debian:12-slim

# Create a non-root user and group
RUN groupadd -r hyperdrive && \
    useradd -r -g hyperdrive -d /hyperdrive-home/home/hyperdrive hyperdrive

RUN apt-get update && \
    apt-get install openssl -y && \
    rm -rf /var/lib/apt/lists/*

# Create directory for hyperdrive and set permissions
RUN mkdir -p /hyperdrive-home/home/hyperdrive && \
    chown -R hyperdrive:hyperdrive /hyperdrive-home

COPY --from=downloader /tmp/download/hyperdrive /bin/hyperdrive
RUN chown hyperdrive:hyperdrive /bin/hyperdrive && \
    chmod 755 /bin/hyperdrive

# Switch to non-root user
USER hyperdrive

WORKDIR /hyperdrive-home

ENTRYPOINT [ "/bin/hyperdrive" ]
CMD [ "/hyperdrive-home" ]

EXPOSE 8080
EXPOSE 9000
