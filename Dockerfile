FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        gcc-riscv64-linux-gnu \
        gdb-multiarch \
        libc6-riscv64-cross \
        libc6-dev-riscv64-cross \
        qemu-user \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/bin/riscv64-linux-gnu-gcc /usr/local/bin/riscv-gcc \
    && ln -sf /usr/bin/qemu-riscv64 /usr/local/bin/qemu-riscv

WORKDIR /workspace
