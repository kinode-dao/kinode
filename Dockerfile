FROM debian:12-slim AS downloader

WORKDIR /tmp/download

RUN apt-get update
RUN apt-get install wget curl openssl jq unzip -y

ADD https://api.github.com/repos/kinode-dao/kinode/releases releases.json
RUN wget "https://github.com/kinode-dao/kinode/releases/download/$(cat releases.json | jq -r '.[0].tag_name')/kinode-x86_64-unknown-linux-gnu.zip"
RUN unzip kinode-x86_64-unknown-linux-gnu.zip

FROM debian:12-slim

RUN apt-get update
RUN apt-get install openssl -y

COPY --from=downloader /tmp/download/kinode /bin/kinode

ENTRYPOINT [ "/bin/kinode" ]
CMD [ "/kinode-home" ]

EXPOSE 8080
EXPOSE 9000