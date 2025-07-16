# s2n-quic-tests

This crate contains integration tests for the s2n-quic implementation. These tests verify the behavior of the QUIC protocol implementation across various scenarios and edge cases.

## Test Organization

The test organization in this crate is inspired by the approach used in the [rust-lang/cargo](https://github.com/rust-lang/cargo) repository. This approach differs from the typical Rust integration test organization described in the [Rust book](https://doc.rust-lang.org/book/ch11-03-test-organization.html) for the reason highlighted in the [Cargo book](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests): 

>Each integration test results in a separate executable binary, and cargo test will run them serially. In some cases this can be inefficient, as it can take longer to compile, and may not make full use of multiple CPUs when running the tests. If you have a lot of integration tests, you may want to consider creating a single integration test, and split the tests into multiple modules.

### Platform-specific Tests

Some tests in this crate are platform-specific, particularly those that depend on s2n-tls, which is only available on Unix systems. These tests are conditionally compiled using `cfg[unix]` attributes. For example:

```rust
#[cfg(unix)]
mod resumption;

#[cfg(not(target_os = "windows"))]
mod mtls;
```

This approach ensures that tests only run on platforms where their dependencies are available. The Cargo.toml file also includes platform-specific dependencies to support this.

## Test Structure

The test suite is organized into several categories:

- **Basic Connectivity Tests**: Verify that clients and servers can establish connections and exchange data.
- **Error Handling Tests**: Ensure proper handling of protocol errors, malformed packets, and connection failures.
- **Network Pathology Tests**: Test behavior under various network conditions (latency, packet loss, reordering).
- **Edge Case Tests**: Verify correct behavior in unusual or extreme scenarios.

### Directory Structure

```
src/
├── lib.rs           # Contains common test utilities and setup functions
└── recorder.rs      # Event recording utilities
tests/
└── testsuite/       # Contains all the test files
    ├── main.rs      # Main test module that imports all tests
    ├── blackhole.rs # Individual test files
    ├── ...          # Other test files
    └── snapshots/   # Snapshot files for tests
```

## Running Tests

### Running All Tests

To run all tests in the crate:

```bash
cargo test
```

### Running Specific Tests

To run tests in a specific module:

```bash
cargo test -- <module_name>
```

For example, to run the blackhole tests:

```bash
cargo test -- blackhole
```

To run a specific test:

```bash
cargo test -- <module_name>::<test_name>
```

For example:

```bash
cargo test -- blackhole::blackhole_success_test
```

### Running Tests with Logging

To enable trace logging during test execution:

```bash
cargo test -- --nocapture
```

## Testing Philosophy

The s2n-quic-tests crate follows several key principles:

1. **Comprehensive Coverage**: Tests cover both normal operation paths and error handling scenarios.

2. **Deterministic Testing**: Tests are designed to be deterministic and reproducible, avoiding flaky tests that could pass or fail randomly.

3. **Isolation**: Each test runs in isolation to prevent interference between tests.

4. **Realistic Scenarios**: Tests simulate real-world network conditions including packet loss, reordering, and latency.

## Test Utilities

### Network Simulation

The test suite uses the [Bach](https://github.com/camshaft/bach) async simulation framework for simulating various network conditions:

- Packet loss
- Packet reordering
- Network latency
- Bandwidth limitations
- Connection blackholes

### Event Recording

The `recorder.rs` module provides utilities for recording and verifying events during test execution, allowing tests to assert on the sequence and content of events.

### Packet Interceptor

The test suite uses a packet interceptor utility that allows tests to inspect, modify, or drop datagrams or modify remote addresses on datagrams as they flow between the client and server. 

Implement the `s2n_quic_core::packet::interceptor::Interceptor` trait and configure it in tests like this:

```rust
let client = Client::builder()
    .with_io(handle.builder().build().unwrap())?  
    .with_packet_interceptor(interceptor)?  
    .start()?;
```

### Common Setup

`src/lib.rs` provides common utilities for setting up test clients and servers with various configurations.

## Contributing New Tests

When adding new tests:

1. Place the test in an appropriate module based on what it's testing
2. Use the common setup utilities when possible
3. Make tests deterministic and avoid dependencies on external systems
4. Document the purpose of the test and any non-obvious aspects
5. Ensure tests run in a reasonable amount of time

## Integration with CI

These tests are automatically run as part of the CI pipeline for s2n-quic. The CI configuration can be found in the `.github/workflows/ci.yml` file in the root of the repository.

