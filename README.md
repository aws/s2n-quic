## s2n-quic ![ci](https://github.com/awslabs/s2n-quic/workflows/ci/badge.svg) ![dependencies](https://github.com/awslabs/s2n-quic/workflows/dependencies/badge.svg) [![codecov](https://codecov.io/gh/awslabs/s2n-quic/branch/main/graph/badge.svg?token=DUSPM9SQW2)](https://codecov.io/gh/awslabs/s2n-quic)

TODO: Fill this README out!

## Installation

`s2n-quic` is available on `crates.io` and can be added to a project like so:

```toml
[dependencies]
s2n-quic = "1"
```

## Security

See [CONTRIBUTING](CONTRIBUTING.md#security-issue-notifications) for more information.

## License

This project is licensed under the Apache-2.0 License.

## Development

### Prerequisites

- GCC / CLang (some CC), Make, and CMake. Installation of these
  items depends on your package manager.
- Install [rustup](https://rustup.rs/)

```sh
# Install components to test and analyze code
rustup component add rustfmt clippy rust-analysis

# Install the nightly toolchain for testing
rustup toolchain install nightly
```

### Initialization

```sh
# Initialize the project's submodules and tell cargo to rebuild it
git submodule update --init
touch tls/s2n-tls-sys/build.rs
```

### Running a fuzz target

You'll need to have `cargo-bolero` installed first.

See Bolero's [instructions](https://camshaft.github.io/bolero/cli-installation.html) to install.

```bash
cargo bolero test varint -p s2n-quic-core -T 30sec --toolchain nightly-2021-09-12 -s address
```

Test targets can be executed on stable by disabling the sanitzer:

```bash
cargo bolero test varint -p s2n-quic-core -s NONE -T 30sec
```

```bash
cargo bolero test "path::manager::fuzz_target::cm_model_test" -p s2n-quic-transport -T 30sec --toolchain nightly-2021-09-12 -s NONE
```

### Testing all the things

You can verify most tests run in the CI locally:

 * Simulate interop tests locally by following the instructions [here](scripts/interop/README.md).
 * Run a compliance report: `./scripts/compliance`
 * Run rustfmt, clippy, and all of the tests: `./scripts/local_test`
