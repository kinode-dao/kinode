FROM --platform=$BUILDPLATFORM alpine AS downloader_start
ARG VERSION
ARG TARGETARCH
WORKDIR /tmp/download
RUN apk update && apk add unzip wget --no-cache

FROM downloader_start AS downloader_amd64
ADD "https://github.com/kinode-dao/kinode/releases/download/${VERSION}/kinode-x86_64-unknown-linux-gnu.zip" kinode-x86_64-unknown-linux-gnu.zip
RUN unzip kinode-x86_64-unknown-linux-gnu.zip

FROM downloader_start AS downloader_arm64
ADD "https://github.com/kinode-dao/kinode/releases/download/${VERSION}/kinode-aarch64-unknown-linux-gnu.zip" kinode-aarch64-unknown-linux-gnu.zip
RUN unzip kinode-aarch64-unknown-linux-gnu.zip

FROM downloader_${TARGETARCH} AS downloader

FROM debian:12-slim

# Create a non-root user and group
RUN groupadd -r kinode && \
    useradd -r -g kinode -d /kinode-home/home/kinode kinode
    #useradd -r -g kinode -d /kinode-home/home/kinode -s /bin/false kinode

RUN apt-get update && \
    apt-get install openssl -y && \
    rm -rf /var/lib/apt/lists/*

# Create directory for kinode and set permissions
RUN mkdir -p /kinode-home/home/kinode && \
    chown -R kinode:kinode /kinode-home

COPY --from=downloader /tmp/download/kinode /bin/kinode
RUN chown kinode:kinode /bin/kinode && \
    chmod 755 /bin/kinode

# Switch to non-root user
USER kinode

WORKDIR /kinode-home

ENTRYPOINT [ "/bin/kinode" ]
CMD [ "/kinode-home" ]

EXPOSE 8080
EXPOSE 9000
