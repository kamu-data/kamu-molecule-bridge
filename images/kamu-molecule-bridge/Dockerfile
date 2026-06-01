FROM docker.io/library/ubuntu:24.04
ARG APP_VERSION

# NOTE: Starting with reqwest 0.13.*, we can no longer provide root certificates via features.
#
#       Therefore, we need to ensure that CA certificates are available at the image level
#       so that all reqwest client instances can function w/o requiring any changes
#       (this is particularly relevant for dependencies that use reqwest).
#
#       More details: https://seanmonstar.com/blog/reqwest-v013-rustls-default/
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/kamu

# Tini
ADD https://github.com/krallin/tini/releases/download/v0.19.0/tini-amd64 /usr/bin/tini
RUN chmod +x /usr/bin/tini

# App
COPY artifacts/kamu-molecule-bridge-x86_64-unknown-linux-gnu.tar.gz /opt/kamu/kamu-molecule-bridge-x86_64-unknown-linux-gnu.tar.gz
RUN tar -xf kamu-molecule-bridge-x86_64-unknown-linux-gnu.tar.gz && \
    chmod +x kamu-molecule-bridge-x86_64-unknown-linux-gnu/kamu-molecule-bridge && \
    mv kamu-molecule-bridge-x86_64-unknown-linux-gnu/kamu-molecule-bridge /opt/kamu/ && \
    rm -rf kamu-molecule-bridge-x86_64-unknown-linux-gnu*


ENTRYPOINT ["/usr/bin/tini", "--"]

CMD ["/opt/kamu/kamu-molecule-bridge"]

EXPOSE 8080/tcp
