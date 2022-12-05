# Kani

`s2n-quic` uses the [Kani Rust Verifier](https://github.com/model-checking/kani) tool for verifying properties throughout various places in the codebase. The Kani test harnesses are written using [bolero](https://github.com/camshaft/bolero/), which is also capable of running them as concrete tests.

## Getting started

First, you will need make sure you install `Rust` and `Kani` on your system.

### Install Rust

The easiest way to install Rust is via [rustup](https://rustup.rs/). Otherwise, check your system's package manager for recommended installation methods.

### Install Kani

Kani is installed with `cargo`:

```sh
$ cargo install kani-verifier
$ cargo-kani setup
```

### Running Kani proofs

After installing `Rust` and `Kani`, you can run the `s2n-quic` proof harnesses. These are currently all located in the `s2n-quic-core` crate:

```sh
$ cd quic/s2n-quic-core
$ cargo kani --tests
```

### Listing Kani proofs

You can find all of the kani harnesses by searching for `kani::proof`

```sh
$ cd quic/s2n-quic-core
$ grep -Rn 'kani::proof' .

< LIST OF LOCATIONS WITH KANI PROOFS >
```
