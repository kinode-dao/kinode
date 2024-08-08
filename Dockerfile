FROM debian:12-slim AS downloader
ARG VERSION

WORKDIR /tmp/download

RUN apt-get update
RUN apt-get install unzip -y

ADD "https://github.com/kinode-dao/kinode/releases/download/${VERSION}/kinode-x86_64-unknown-linux-gnu.zip" kinode-x86_64-unknown-linux-gnu.zip
RUN unzip kinode-x86_64-unknown-linux-gnu.zip

FROM debian:12-slim

RUN apt-get update
RUN apt-get install openssl -y

COPY --from=downloader /tmp/download/kinode /bin/kinode

ENTRYPOINT [ "/bin/kinode" ]
CMD [ "/kinode-home" ]

EXPOSE 8080
EXPOSE 9000