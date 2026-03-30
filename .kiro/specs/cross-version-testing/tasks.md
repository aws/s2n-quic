# Implementation Plan

- [ ] 1. Add previous version dependencies to Cargo.toml
  - [x] 1.1 Add `s2n-quic-prev`, `s2n-quic-core-prev`, and `s2n-codec-prev` renamed dependencies to `quic/s2n-quic-tests/Cargo.toml`
    - Add `s2n-quic-prev = { package = "s2n-quic", version = "1.74", features = [...] }` with the same feature flags as the current `s2n-quic` dependency
    - Add `s2n-quic-core-prev = { package = "s2n-quic-core", version = "0.74", features = ["testing"] }`
    - Add `s2n-codec-prev = { package = "s2n-codec", version = "0.74" }`
    - Add `proptest` dev-dependency for property-based testing
    - Verify the crate compiles with `cargo check -p s2n-quic-tests`
    - _Requirements: 3.1, 3.2_

- [ ] 2. Implement VersionConfig enum and serialization
  - [ ] 2.1 Create `VersionConfig` enum with `Display` and `FromStr` implementations in `quic/s2n-quic-tests/src/lib.rs`
    - Implement `SameVersion`, `ClientAhead`, `ServerAhead` variants
    - Implement `Display` producing `"same-version"`, `"client-ahead"`, `"server-ahead"`
    - Implement `FromStr` parsing those strings back, returning `Err` for unknown strings
    - _Requirements: 6.1, 6.2_
  - [ ]* 2.2 Write property test for VersionConfig round-trip serialization
    - **Property 1: VersionConfig round-trip serialization**
    - **Validates: Requirements 6.1, 6.2**
    - Use `proptest` crate, generate arbitrary `VersionConfig` values, assert `value == VersionConfig::from_str(&value.to_string()).unwrap()`
    - Run a minimum of 100 iterations
  - [ ]* 2.3 Write unit tests for VersionConfig
    - Test `Display` output for each variant
    - Test `FromStr` for valid and invalid inputs
    - _Requirements: 6.1, 6.2_

- [ ] 3. Implement previous version helper types in lib.rs
  - [ ] 3.1 Implement `PrevBlocklistSubscriber` that implements `s2n_quic_prev::provider::event::Subscriber`
    - Mirror the existing `BlocklistSubscriber` but use types from `s2n_quic_prev` and `s2n_quic_core_prev`
    - _Requirements: 5.1, 5.2_
  - [ ] 3.2 Implement `PrevRandom` that implements `s2n_quic_prev::provider::random::Provider` and `Generator`
    - Mirror the existing `Random` struct but implement the prev version's traits
    - _Requirements: 5.1, 5.2_
  - [ ] 3.3 Add `PREV_SERVER_CERTS` static and `prev_tracing_events` function
    - `PREV_SERVER_CERTS` uses certificates from `s2n_quic_core_prev::crypto::tls::testing::certificates`
    - `prev_tracing_events` returns an `impl s2n_quic_prev::provider::event::Subscriber`
    - _Requirements: 5.1, 5.3_

- [ ] 4. Implement previous version helper functions in lib.rs
  - [ ] 4.1 Implement `prev_build_server`, `prev_start_server`, and `prev_server` functions
    - Mirror `build_server`, `start_server`, and `server` but use `s2n_quic_prev::Server` and prev helper types
    - `prev_start_server` accepts `s2n_quic_prev::Server`, spawns accept loop, returns `SocketAddr`
    - _Requirements: 2.1, 2.2, 5.1_
  - [ ] 4.2 Implement `prev_build_client`, `prev_start_client`, and `prev_client` functions
    - Mirror `build_client`, `start_client`, and `client` but use `s2n_quic_prev::Client` and prev helper types
    - `prev_start_client` accepts `s2n_quic_prev::Client`, spawns connect/send/recv logic, returns `Result`
    - _Requirements: 2.1, 2.2, 5.1_
  - [ ] 4.3 Implement prev mTLS helpers (`prev_build_client_mtls_provider`, `prev_build_server_mtls_provider`)
    - Mirror the existing mTLS helpers using `s2n_quic_prev::provider::tls` types
    - Gate behind `cfg(not(target_os = "windows"))` like the current versions
    - _Requirements: 5.3_

- [ ] 5. Checkpoint - Make sure all tests are passing
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Implement the `compat_test!` macro
  - [ ] 6.1 Create the `compat_test!` macro in `quic/s2n-quic-tests/src/lib.rs` (or a dedicated `macros.rs` module)
    - The macro takes a test name and body block
    - Generates three sub-modules: `same_version`, `client_ahead`, `server_ahead`
    - Each sub-module has different `use` statements that swap `Server`, `Client`, `Connect`, helper functions, `SERVER_CERTS`, `Random`, `BlocklistSubscriber`, `certificates`, `Data`, and other types to the appropriate version
    - Each sub-module contains a `#[test] fn test()` with the pasted body
    - _Requirements: 1.1, 1.2, 1.3, 2.1, 2.2, 2.3, 4.2, 8.1, 8.2_

- [ ] 7. Migrate the first test (`self_test`) to use `compat_test!`
  - [ ] 7.1 Convert `self_test::client_server_test` to use `compat_test!`
    - Replace `#[test] fn client_server_test()` with `compat_test!(client_server_test { ... })`
    - Verify the test generates three sub-tests: `same_version::test`, `client_ahead::test`, `server_ahead::test`
    - Run `cargo test -p s2n-quic-tests client_server_test` and verify all three pass
    - _Requirements: 1.1, 2.1, 2.2, 2.3, 8.1_

- [ ] 8. Checkpoint - Make sure all tests are passing
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 9. Migrate simple helper-based tests to `compat_test!`
  - [ ] 9.1 Convert `blackhole` tests to use `compat_test!`
    - Both `blackhole_success_test` and `blackhole_failure_test` use vanilla `server()`/`client()` helpers
    - _Requirements: 1.1, 5.1_
  - [ ] 9.2 Convert `issue_1427::tokio_read_exact_test` to use `compat_test!`
    - Uses `server()` and `build_client()` helpers
    - _Requirements: 1.1, 5.1_

- [ ] 10. Migrate custom-builder tests to `compat_test!`
  - [ ] 10.1 Convert `interceptor` tests to use `compat_test!`
    - Uses custom `Server::builder()` with `with_packet_interceptor`
    - Verify the macro correctly swaps `Server`, `Loss`, `Random` types
    - _Requirements: 1.2, 5.2_
  - [ ] 10.2 Convert `issue_954::client_path_handle_update` to use `compat_test!`
    - Uses custom `Server::builder()` and `Client::builder()` with event subscribers
    - _Requirements: 1.2, 5.2_
  - [ ] 10.3 Convert `issue_1361::stream_reset_test` to use `compat_test!`
    - Uses custom `Server::builder()` with `with_limits`
    - _Requirements: 1.2, 5.2_
  - [ ] 10.4 Convert `issue_1464::local_stream_open_notify_test` to use `compat_test!`
    - Uses `build_server()` and `build_client()` helpers
    - _Requirements: 1.2, 5.2_
  - [ ] 10.5 Convert `issue_1717::increasing_pto_count_under_loss` to use `compat_test!`
    - Uses custom `Server::builder()` and `Client::builder()` with event subscribers
    - _Requirements: 1.2, 5.2_

- [ ] 11. Checkpoint - Make sure all tests are passing
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 12. Migrate MTU tests to `compat_test!`
  - [ ] 12.1 Convert `mtu` tests to use `compat_test!`
    - The `mtu_test!` macro generates sub-tests with custom builder chains including MTU config, event recorders, and TLS providers
    - May need to compose `compat_test!` with `mtu_test!` or refactor the `mtu_test!` macro
    - Standalone tests (`mtu_loss_no_blackhole`, `mtu_blackhole`, `minimum_initial_packet`) also need conversion
    - _Requirements: 1.2, 5.2_

- [ ] 13. Migrate TLS-specific tests to `compat_test!`
  - [ ] 13.1 Convert `mtls` tests to use `compat_test!`
    - Uses `build_server_mtls_provider` and `build_client_mtls_provider` — macro must swap to prev versions
    - _Requirements: 1.2, 5.3_
  - [ ] 13.2 Convert `slow_tls` test to use `compat_test!`
    - Uses `SlowTlsProvider` wrapping default TLS endpoints
    - _Requirements: 1.2, 5.3_
  - [ ] 13.3 Convert `fips` test to use `compat_test!`
    - Uses custom TLS security policy configuration
    - _Requirements: 1.2, 5.3_

- [ ] 14. Migrate event subscriber and advanced tests to `compat_test!`
  - [ ] 14.1 Convert `platform_events` test to use `compat_test!`
    - Uses custom event subscribers on both sides
    - _Requirements: 1.2, 5.2_
  - [ ] 14.2 Convert `handshake_cid_rotation` tests to use `compat_test!`
    - Uses custom `connection_id` configuration on both sides
    - _Requirements: 1.2, 5.2_
  - [ ] 14.3 Convert `skip_packets` tests to use `compat_test!`
    - Uses custom event subscribers and packet interceptors
    - _Requirements: 1.2, 5.2_
  - [ ] 14.4 Convert `connection_migration` tests to use `compat_test!`
    - Uses custom interceptors, `on_socket` callbacks, event recorders
    - _Requirements: 1.2, 5.2_

- [ ] 15. Checkpoint - Make sure all tests are passing
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 16. Migrate remaining tests and identify opt-outs
  - [ ] 16.1 Convert remaining convertible tests to `compat_test!`
    - `connection_limits`, `endpoint_limits`, `exporter`, `chain`, `client_handshake_confirm`, `pto`, `deduplicate`, `initial_rtt`
    - _Requirements: 1.2, 5.2_
  - [ ] 16.2 Identify and document tests that opt out of cross-version testing
    - Tests that use version-specific internal APIs (e.g., `dc`, `no_tls`, `tls_context`, `offload`, `buffer_limit`, `resumption`) remain as `#[test]` only
    - Leave these as-is — opting out is simply not using `compat_test!`
    - _Requirements: 4.1, 4.2_

- [ ] 17. Implement version staleness check
  - [ ] 17.1 Add a test that verifies `s2n-quic-prev` version is older than current
    - Compare current crate version against the prev version
    - Fail with a descriptive message if they are equal
    - _Requirements: 7.1, 7.2_
  - [ ]* 17.2 Write property test for version staleness detection
    - **Property 2: Version staleness detection**
    - **Validates: Requirements 7.1, 7.2**
    - Use `proptest` to generate pairs of semver versions, assert the staleness check function returns the correct result based on ordering
    - Run a minimum of 100 iterations

- [ ] 18. Final Checkpoint - Make sure all tests are passing
  - Ensure all tests pass, ask the user if questions arise.
