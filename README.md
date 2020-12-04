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

- Install [rustup](https://rustup.rs/)
- Run `rustup component add rustfmt clippy rls rust-analysis`

### Running a fuzz target

You'll need to have `cargo-bolero` installed first.

See Bolero's [instructions](https://camshaft.github.io/bolero/cli-installation.html) to install.

```bash
$ cargo bolero test varint -p s2n-quic-core -s address
```

Test targets can be executed on stable by disabling the sanitzer:

```bash
$ cargo bolero test varint -p s2n-quic-core -s NONE
```

### Testing all the things

cargo test

cargo clippy --all-features --all-targets -- -D warnings
cargo +nightly run --release --bin cargo-compliance -- report --spec-pattern 'specs/**/*.toml' --source-pattern 'quic/**/*.rs' --workspace --exclude compliance --exclude cargo-compliance --html target/compliance/coverage.html


### Docker
cd qns
cp ../target/debug/s2n-quic-qns s2n-quic-qns
cp ../target/debug/s2n-quic-qns s2n-quic-qns-release
cp ../target/debug/s2n-quic-qns s2n-quic-qns-debug
sudo docker build . --file ../.github/interop/Dockerfile --tag awslabs/s2n-quic-qns
