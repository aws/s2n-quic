# s2n-quic-xdp

## Prerequisites

1. Install a rust stable toolchain: `rustup install stable`
1. Install a rust nightly toolchain: `rustup install nightly`
1. Install bpf-linker: `cargo install bpf-linker`

## Build eBPF

```bash
cargo xtask build-ebpf
```

To perform a release build you can use the `--release` flag.

## Disassemble eBPF Program

```bash
cargo xtask disasm
```

## Run the kernel verifier

```bash
RUST_LOG=trace cargo xtask run -- -i lo --trace
```
