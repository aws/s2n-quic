# s2n-quic-xdp

AF_XDP IO provider for s2n-quic endpoints.

This crate provides the low-level building blocks needed to drive an
[s2n-quic](https://github.com/aws/s2n-quic) endpoint with [AF_XDP](https://docs.kernel.org/networking/af_xdp.html)
sockets instead of the standard kernel UDP path. It exposes UMEM allocation,
ring buffers, AF_XDP socket creation, the syscall layer, and the default eBPF
program used to steer QUIC traffic into userspace.

## Status

The crate intentionally exposes primitives rather than a single high-level
"build me an AF_XDP provider" helper. As described in the original
integration PR ([#1765](https://github.com/aws/s2n-quic/pull/1765)), an
"easy mode" was deferred to gather real-world feedback before settling on a
public API shape:

> applications will need to use this code as a starting point. We may add
> an "easy mode" in the future to simplify this integration but I think it's
> best to unblock and get some feedback first.

Until that lands, the canonical reference for wiring the primitives in this
crate into a working `s2n_quic::provider::io::Provider` is
[`quic/s2n-quic-qns/src/xdp.rs`](../../../quic/s2n-quic-qns/src/xdp.rs).
Both server and client setup live there and can be adapted as a starting
point.

## What you get from this crate

- `bpf` — the default eBPF/XDP program (port-map + XSK-map based steering)
- `if_xdp`, `umem`, `ring`, `socket`, `syscall` — the AF_XDP kernel API surface
- `io` — `Tx`/`Rx` channel types implementing the `s2n_quic_core::io` traits
- `mmap` — helpers for mmap'd memory regions

To consume this crate from `s2n-quic`, enable the `unstable-provider-io-xdp`
feature on `s2n-quic` and import via `s2n_quic::provider::io::xdp`.

## Requirements

- Linux with AF_XDP support (5.x or later).
- `CAP_NET_ADMIN` and `CAP_BPF` to load the eBPF program and bind XSKs.
- Git LFS. The compiled eBPF object files under `src/bpf/` are stored in LFS:

  ```sh
  git lfs install
  git lfs pull
  ```

  Without LFS, `bpf::DEFAULT_PROGRAM` resolves to a 129-byte LFS pointer and
  loading fails with `ParseError(ElfError("Unknown file magic"))`.

## Running the qns reference

A minimal end-to-end smoke test using the qns reference wiring on a veth
pair across a network namespace:

```sh
# multi-queue veth pair (see "Callouts" below for why)
sudo ip link add veth0 numrxqueues 16 numtxqueues 16 \
    type veth peer name veth1 numrxqueues 16 numtxqueues 16

sudo ip netns add xdpns
sudo ip link set veth1 netns xdpns

sudo ip addr add 10.99.0.1/24 dev veth0
sudo ip link set veth0 up

sudo ip netns exec xdpns ip addr add 10.99.0.2/24 dev veth1
sudo ip netns exec xdpns ip link set veth1 up
sudo ip netns exec xdpns ip link set lo up
```

Build qns with the XDP feature, then run the server in the netns and the
client on the host using the same flags exposed by `quic/s2n-quic-qns/src/xdp.rs`
(`--interface`, `--xdp-mode`, `--port`, etc.). Cleanup:

```sh
sudo ip link del veth0
sudo ip netns del xdpns
```

## Callouts

- **AF_XDP on `lo` doesn't reliably round-trip locally generated traffic.**
  Use a veth pair (as above) for local testing.

- **`syscall::max_queues` returns the interface's pre-set max channel count,
  not the active queue count.** veth defaults to a max of 16 channels but
  only 1 active queue, so binding an AF_XDP socket on queue 1 fails with
  `EINVAL`. Either create the veth with `numrxqueues 16 numtxqueues 16` (as
  above) or bring up additional queues with `ethtool -L <iface> rx N tx N`.

- **`unstable-provider-io-xdp` is unstable.** The API surface, including
  the high-level "easy mode" referenced above, may change.

## Developing this crate

See [`../README.md`](../README.md) for instructions on building the eBPF
program, disassembling it, and running it through the kernel verifier.
