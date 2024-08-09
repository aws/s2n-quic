# s2n-quic Continuous Integration

`s2n-quic` executes a comprehensive suite of tests, linters, benchmarks, simulations and other checks on [each pull request](https://github.com/aws/s2n-quic/actions/workflows/ci.yml?query=event%3Apull_request) and merge into [main](https://github.com/aws/s2n-quic/actions/workflows/ci.yml?query=branch%3Amain). Information about these checks is provided below.

## Tests

`s2n-quic` defines many tests that can be executed with `cargo test`. These tests, described below, are executed across a variety of operating systems, architectures, and Rust versions.

#### Unit Tests

Unit tests validate the expected behavior of individual components of `s2n-quic`. Typically, unit tests will be located in a `tests` module at the end of the file being tested. When there are a significant number of unit test functions for given component, the `tests` module may be located in a separate file. 

#### Integration Tests

`s2n-quic` integration tests use the public API to validate the end-to-end behavior of the library under specific scenarios and configurations. Integration tests are located in the top-level `s2n-quic` crate, in the [tests module](https://github.com/aws/s2n-quic/tree/main/quic/s2n-quic/src/tests). 

#### Snapshot Tests

Snapshot tests use [insta](https://crates.io/crates/insta) to assert complex output remains consistent with expected references values. In `s2n-quic`, snapshot tests are typically constructed in one of two ways:
  * using the `insta::assert_debug_snapshot` macro to compare the `Debug` representation of an instance to the snapshot
  * using the `event::testing::Publisher::snapshot()` event publisher to assert against a snapshot of events emitted by the `s2n-quic` [event framework](https://docs.rs/s2n-quic/latest/s2n_quic/provider/event/trait.Event.html) 

#### Property Tests

Property tests assert that specific properties of the output of functions and components are upheld under a variety of inputs. `s2n-quic` uses the [Bolero property-testing framework](https://camshaft.github.io/bolero/introduction.html) to execute property-testing with multiple fuzzing engines as well as the [Kani Rust Verifier](https://model-checking.github.io/kani/). For more details on how Bolero and Kani are used together in `s2n-quic`, see the following blog posts from the [Kani Rust Verifier Blog](https://model-checking.github.io/kani-verifier-blog/):
  * [From Fuzzing to Proof: Using Kani with the Bolero Property-Testing Framework](https://model-checking.github.io/kani-verifier-blog/2022/10/27/using-kani-with-the-bolero-property-testing-framework.html)
  * [How s2n-quic uses Kani to inspire confidence](https://model-checking.github.io/kani-verifier-blog/2023/05/30/how-s2n-quic-uses-kani-to-inspire-confidence.html)

#### Fuzz Tests

Fuzz tests provide large amounts of varied input data to assert `s2n-quic` behaves as expected regardless of the input. Fuzz testing in `s2n-quic` comes in three flavors:
  * Component-level fuzz testing using the [Bolero property-testing framework](https://camshaft.github.io/bolero/introduction.html). These tests generate a corpus of inputs that is included in the `s2n-quic` repository to allow the fuzz tests to be replayed when executing `cargo test`. 
  * End-to-end QUIC protocol-level fuzzing using [quic-attack](https://github.com/aws/s2n-quic/blob/main/scripts/quic-attack/README.md). `quic-attack` is a collection of features that collectively turn `s2n-quic` into an online QUIC protocol fuzzer. It allows incoming and outgoing datagrams and packets, as well as the port number on incoming datagrams, to be intercepted and manipulated. 
  * UDP protocol-level fuzzing using [udp-attack](https://github.com/aws/s2n-quic/tree/main/tools/udp-attack). `udp-attack` generates random UDP packets and transmits them to `s2n-quic` to catch issues with packet handling.

#### Concurrency permutation testing

[loom](https://crates.io/crates/loom) is used to validate the behavior of concurrent code in `s2n-quic`. `loom` executes concurrent code using a simulation of the operating system scheduler and Rust memory model to evaluate concurrent code under all possible thread interleavings.

## Interoperability

The [quic-interop-runner](https://github.com/marten-seemann/quic-interop-runner) defines a suite of test cases that ensure compatibility between QUIC implementations. `s2n-quic` publishes [a report](https://dnglbrstg7yg.cloudfront.net/08c33571ee8679775e810303f65c96c1d48e270d/interop/index.html) with the results.

## Compliance

`s2n-quic` annotates source code with inline references to requirements in [IETF RFC](https://www.ietf.org/process/rfcs/) specifications. [Duvet](https://github.com/awslabs/duvet) is used to generate [a report](https://dnglbrstg7yg.cloudfront.net/42ee277272a079c49b4f2bbd034a3547116d71a5/interop/index.html), which makes it easy to track compliance with each requirement.

## Simulations

A Monte Carlo simulation tool is used to execute thousands of randomized simulations of `s2n-quic` that vary one or more network variables, such as bandwidth, jitter, and round trip time. The [report output](https://dnglbrstg7yg.cloudfront.net/ab9723a772f03a9793c9863e73c9a48fab3c5235/sim/index.html) provides a visual representation of the relationship between the input variables and overall performance.

A loss recovery simulation tool plots the growth of the congestion window over time under various simulated loss scenarios and publishes the results in [a report](https://dnglbrstg7yg.cloudfront.net/ab9723a772f03a9793c9863e73c9a48fab3c5235/recovery-simulations/index.html) to visualize changes to the congestion control algorithm.

## Performance & Efficiency Profiling

[Flame graphs](https://www.brendangregg.com/flamegraphs.html) are generated and published in a [report](https://dnglbrstg7yg.cloudfront.net/a9b5f7d1a688770e71ee6967699848bb616b79e6/perf/index.html) to visualize stack traces produced under a variety of data transfer scenarios. 

[dhat](https://crates.io/crates/dhat) performs heap profiling and publishes the results in [a report](https://dnglbrstg7yg.cloudfront.net/dhat/dh_view.html?url=/ab9723a772f03a9793c9863e73c9a48fab3c5235/dhat/dhat-heap.json). 

## Clippy

[clippy](https://github.com/rust-lang/rust-clippy) is a rust linter which catches common mistakes.

## Rustfmt

[rustfmt](https://github.com/rust-lang/rustfmt) ensures code is consistently formatted.

## Miri

[Miri](https://github.com/rust-lang/miri) detects [undefined behavior](https://doc.rust-lang.org/reference/behavior-considered-undefined.html) and memory leaks.

## Code Coverage

[LLVM source-based code coverage](https://llvm.org/docs/CommandGuide/llvm-cov.html) measures how much of `s2n-quic` code is executed by tests. `s2n-quic` publishes [a report](https://dnglbrstg7yg.cloudfront.net/ab9723a772f03a9793c9863e73c9a48fab3c5235/coverage/index.html) with the results.
