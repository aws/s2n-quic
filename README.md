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
$ cargo bolero fuzz varint -p s2n-quic-core -s address
```

Fuzz targets can be executed on stable by removing the sanitzer flag:

```bash
$ cargo bolero fuzz varint -p s2n-quic-core
```
