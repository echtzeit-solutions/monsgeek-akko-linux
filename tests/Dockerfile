# MonsGeek M1 V5 HE Driver - Test Container
# Build: docker build -t monsgeek-test tests/
# Run:   docker run --rm -v $(pwd):/workspace monsgeek-test make test-build

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_HOME=/root/.cargo
ENV RUSTUP_HOME=/root/.rustup
ENV PATH="/root/.cargo/bin:${PATH}"

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential pkg-config \
    libudev-dev libhidapi-dev \
    protobuf-compiler \
    libasound2-dev libdbus-1-dev \
    libjack-jackd2-dev \
    libclang-dev libelf-dev zlib1g-dev \
    curl git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . "$CARGO_HOME/env" \
    && rustup install stable nightly \
    && rustup default stable \
    && rustup component add rustfmt clippy \
    && rustup component add rust-src --toolchain nightly

# Install bpf-linker (for eBPF builds)
RUN . "$CARGO_HOME/env" && cargo install bpf-linker

WORKDIR /workspace

# Default command: run build tests
CMD ["./tests/run_tests.sh", "build"]
