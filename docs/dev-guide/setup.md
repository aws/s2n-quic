# Setup


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
git submodule update --init && touch tls/s2n-tls-sys/build.rs
```
