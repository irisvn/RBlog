# RBlog's image: the binary built by .github/workflows/release.yml (dist/<arch>/rblog, the admin
# panel is built in), its configuration, and an entrypoint that migrates before serving.
# Nothing is compiled here. Locally: build dist/amd64/rblog (or arm64) first.
# The same distribution as the release runners (ubuntu-24.04): the binary links its glibc (2.39).
FROM ubuntu:24.04
ARG TARGETARCH
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/*
ARG RBLOG_VERSION=""
ARG RBLOG_COMMIT=""
LABEL org.opencontainers.image.title="RBlog" \
      org.opencontainers.image.source="https://github.com/irisvn/RBlog" \
      org.opencontainers.image.version="${RBLOG_VERSION}" \
      org.opencontainers.image.revision="${RBLOG_COMMIT}"
WORKDIR /app
COPY dist/${TARGETARCH}/rblog /app/rblog
COPY config /app/config
COPY docker/entrypoint.sh /app/entrypoint.sh
RUN mkdir -p /app/storage && chmod 0755 /app/entrypoint.sh /app/rblog
ENV APP_ENV=production \
    APP_DEBUG=false \
    RUSTCMS_APP__LISTEN=0.0.0.0:8000
EXPOSE 8000
ENTRYPOINT ["/usr/bin/tini", "--", "/app/entrypoint.sh"]
CMD ["serve"]
