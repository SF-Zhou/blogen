# syntax=docker/dockerfile:1

# blogen Linux releases are glibc binaries, so the image is a thin wrapper
# around a prebuilt executable on a glibc base. Build it with:
#
#   mkdir -p .docker-bin/amd64 .docker-bin/arm64
#   tar -xzf blogen-<version>-x86_64-unknown-linux-gnu.tar.gz -C .docker-bin/amd64 blogen
#   tar -xzf blogen-<version>-aarch64-unknown-linux-gnu.tar.gz -C .docker-bin/arm64 blogen
#   docker buildx build --platform linux/amd64,linux/arm64 -t blogen .
#
# The release workflow does this automatically and pushes the image to
# ghcr.io/sf-zhou/blogen.

FROM debian:bookworm-slim

ARG TARGETARCH=amd64
ARG BLOGEN_VERSION=dev

LABEL org.opencontainers.image.title="blogen" \
      org.opencontainers.image.description="Fast, themeable static blog generator" \
      org.opencontainers.image.source="https://github.com/SF-Zhou/blogen" \
      org.opencontainers.image.url="https://github.com/SF-Zhou/blogen" \
      org.opencontainers.image.version="${BLOGEN_VERSION}" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY .docker-bin/${TARGETARCH}/blogen /usr/local/bin/blogen
RUN chmod 0755 /usr/local/bin/blogen

WORKDIR /site
ENTRYPOINT ["blogen"]
CMD ["build"]
