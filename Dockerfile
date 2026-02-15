FROM ubuntu:24.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /usr/sbin/nologin -d /data bobbin

COPY target/release/bobbin /usr/local/bin/bobbin

RUN mkdir -p /data && chown bobbin:bobbin /data

USER bobbin
WORKDIR /data

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -sf http://localhost:3000/status || exit 1

ENTRYPOINT ["bobbin", "serve", "--http", "--port", "3000", "."]
