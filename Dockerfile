# syntax=docker/dockerfile:1

# blogen is distributed as a static Linux binary (musl), so the image is a
# thin wrapper around a prebuilt executable. Build it with:
#
#   mkdir -p .docker-bin/amd64 .docker-bin/arm64
#   tar -xzf blogen-<version>-x86_64-unknown-linux-musl.tar.gz -C .docker-bin/amd64 blogen
#   tar -xzf blogen-<version>-aarch64-unknown-linux-musl.tar.gz -C .docker-bin/arm64 blogen
#   docker buildx build --platform linux/amd64,linux/arm64 -t blogen .
#
# The release workflow does this automatically and pushes the image to
# ghcr.io/sf-zhou/blogen.

FROM alpine:3.22

ARG TARGETARCH=amd64
ARG BLOGEN_VERSION=dev

LABEL org.opencontainers.image.title="blogen" \
      org.opencontainers.image.description="Fast, themeable static blog generator" \
      org.opencontainers.image.source="https://github.com/SF-Zhou/blogen" \
      org.opencontainers.image.url="https://github.com/SF-Zhou/blogen" \
      org.opencontainers.image.version="${BLOGEN_VERSION}" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

RUN apk add --no-cache ca-certificates

COPY .docker-bin/${TARGETARCH}/blogen /usr/local/bin/blogen
RUN chmod 0755 /usr/local/bin/blogen

WORKDIR /site
ENTRYPOINT ["blogen"]
CMD ["build"]
