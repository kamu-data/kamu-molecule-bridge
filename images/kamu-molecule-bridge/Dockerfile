FROM docker.io/library/ubuntu:24.04
ARG APP_VERSION

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
