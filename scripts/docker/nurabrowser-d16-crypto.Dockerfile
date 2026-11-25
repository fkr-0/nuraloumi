# Host-only source-pinned ARMv7/musl crypto preflight toolchain.
# Build: docker build --platform linux/arm/v7 --pull=false \
#   -f scripts/docker/nurabrowser-d16-crypto.Dockerfile \
#   -t nuraloumi-crypto-d16:alpine322-r2 scripts/docker
# Compiler inputs and APK versions must be recorded alongside the output.
FROM alpine:3.22

RUN apk add --no-cache \
    binutils \
    build-base \
    coreutils \
    findutils \
    gmp-dev \
    libgpg-error-dev \
    linux-headers \
    m4 \
    patch \
    perl \
    texinfo
