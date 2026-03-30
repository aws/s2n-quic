# Requirements Document

## Introduction

The s2n-quic integration test suite (`s2n-quic-tests`) uses the bach discrete event simulator to test client-server QUIC scenarios. Currently, all tests run with the same version of s2n-quic on both client and server. To catch cross-version incompatibilities early, each test should automatically run in three version configurations: same-version on both sides, client ahead of server, and server ahead of client. The solution should minimize the additional work a developer needs to do when writing new tests — cross-version testing should "just work" with the existing test patterns.

## Glossary

- **Test Harness**: The shared infrastructure in `s2n-quic-tests` that configures and runs client-server integration tests using the bach simulator.
- **Version Configuration**: A pairing of s2n-quic library versions used for the client and server in a single test run. There are three configurations: same-version, client-ahead, and server-ahead.
- **Current Version**: The s2n-quic code in the local working tree, pull request branch, or main branch — not necessarily a published release.
- **Previous Version**: The last published release of s2n-quic on crates.io.
- **Client-Ahead Configuration**: A version configuration where the client uses the current version of s2n-quic and the server uses the previous version.
- **Server-Ahead Configuration**: A version configuration where the server uses the current version of s2n-quic and the client uses the previous version.
- **Same-Version Configuration**: A version configuration where both client and server use the current version of s2n-quic.
- **Builder Abstraction**: A trait or function interface that constructs a `Client` or `Server` from the test harness `Handle` and `Model`, abstracting over which version of s2n-quic is used.
- **Test Matrix**: The set of version configurations over which a single logical test is executed.
- **bach Simulator**: The discrete event simulator used by the test harness to drive network I/O without real sockets.

## Requirements

### Requirement 1

**User Story:** As a developer writing new integration tests, I want cross-version testing to apply automatically, so that I do not need to write additional code or configuration for each new test.

#### Acceptance Criteria

1. WHEN a developer writes a new integration test using the standard test harness builder functions, THE Test Harness SHALL execute that test across all three version configurations without additional developer effort.
2. WHEN a developer writes a test that uses custom `Client::builder()` or `Server::builder()` calls, THE Test Harness SHALL provide a mechanism (such as a macro or trait) to opt that test into cross-version execution with minimal code changes.
3. WHEN a test is executed across version configurations, THE Test Harness SHALL report each configuration as a distinct test case so that failures identify which version configuration failed.

### Requirement 2

**User Story:** As a developer, I want the test harness to support building a client and server from two different versions of the s2n-quic library, so that cross-version scenarios can be tested.

#### Acceptance Criteria

1. WHEN the Test Harness builds a client-ahead configuration, THE Test Harness SHALL construct the client using the current version (local working tree) and the server using the previous version.
2. WHEN the Test Harness builds a server-ahead configuration, THE Test Harness SHALL construct the server using the current version (local working tree) and the client using the previous version.
3. WHEN the Test Harness builds a same-version configuration, THE Test Harness SHALL construct both client and server using the current version (local working tree).

### Requirement 3

**User Story:** As a CI maintainer, I want the previous version of s2n-quic to be resolved and built automatically, so that cross-version tests do not require manual version management.

#### Acceptance Criteria

1. WHEN the cross-version test suite is built, THE build system SHALL resolve the previous version as the last published release of s2n-quic on crates.io.
2. WHEN the previous version dependency is updated (e.g., after a new release), THE build system SHALL require a change to a single, clearly identified configuration point (such as a version string in a Cargo.toml dependency entry).

### Requirement 4

**User Story:** As a developer, I want to opt specific tests out of cross-version testing, so that tests which exercise version-specific features or internal APIs run only in the same-version configuration.

#### Acceptance Criteria

1. WHEN a test is annotated with an opt-out marker, THE Test Harness SHALL execute that test only in the same-version configuration.
2. WHEN a test is not annotated with an opt-out marker, THE Test Harness SHALL execute that test across all three version configurations.

### Requirement 5

**User Story:** As a developer, I want the builder abstraction to support the full range of existing test patterns (simple helper-based tests, custom builder tests, tests with event recorders, tests with packet interceptors), so that cross-version testing covers the existing test suite.

#### Acceptance Criteria

1. WHEN a test uses the simple `server()` and `client()` helper functions, THE Builder Abstraction SHALL support constructing those helpers from either version of s2n-quic.
2. WHEN a test uses custom `Server::builder()` chains with event subscribers, packet interceptors, or MTU configuration, THE Builder Abstraction SHALL support constructing those builders from either version of s2n-quic.
3. WHEN a test uses TLS-specific configuration (mTLS, resumption, slow TLS), THE Builder Abstraction SHALL support constructing those TLS providers from either version of s2n-quic.

### Requirement 6

**User Story:** As a developer, I want the cross-version test infrastructure to serialize and deserialize version configuration metadata, so that test reports and CI systems can identify which configuration was tested.

#### Acceptance Criteria

1. WHEN a test runs in a specific version configuration, THE Test Harness SHALL produce a human-readable label (such as "same-version", "client-ahead", "server-ahead") identifying the configuration.
2. WHEN the version configuration label is serialized to a string and then parsed back, THE Test Harness SHALL produce an equivalent version configuration value.

### Requirement 7

**User Story:** As a developer, I want a test to fail if the previous version dependency becomes stale after a new release, so that cross-version testing always covers the correct version pair.

#### Acceptance Criteria

1. WHEN the current s2n-quic version is strictly greater than the previous version dependency, THE version staleness test SHALL pass.
2. WHEN the current s2n-quic version equals the previous version dependency (indicating the prev dependency was not updated after a release), THE version staleness test SHALL fail with a descriptive message.

### Requirement 8

**User Story:** As a developer, I want to run all integration tests including cross-version tests with a single `cargo test` invocation, so that I do not need special command-line flags or separate test commands.

#### Acceptance Criteria

1. WHEN a developer runs `cargo test` in the test crate, THE Test Harness SHALL execute all integration tests across all version configurations without requiring additional command-line arguments or environment variables.
2. WHEN a developer runs `cargo test` with a test name filter, THE Test Harness SHALL execute only the matching tests across all applicable version configurations.

