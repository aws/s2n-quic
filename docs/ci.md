# s2n-quic Continuous Integration

`s2n-quic` runs many tests on [each pull request](https://github.com/aws/s2n-quic/actions/workflows/ci.yml?query=event%3Apull_request) and merge into [main](https://github.com/aws/s2n-quic/actions/workflows/ci.yml?query=branch%3Amain). This ensures each change is thoroughly tested.

## Tests

`s2n-quic` defines many tests that can be executed with `cargo test`. These tests include unit, integration, snapshot, property, and fuzz tests.

## Clippy

[clippy](https://github.com/rust-lang/rust-clippy) is a rust linter which catches common mistakes.

## Rustfmt

[rustfmt](https://github.com/rust-lang/rustfmt) ensures code is consistently formatted.

## Interop

The [quic-interop-runner](https://github.com/marten-seemann/quic-interop-runner) defines many test cases that ensure many of the 3rd party QUIC implementations are compatible. `s2n-quic` publishes [a report](https://dnglbrstg7yg.cloudfront.net/08c33571ee8679775e810303f65c96c1d48e270d/interop/index.html) with the results.

## Compliance

`s2n-quic` annotates source code with inline references to requirements in design documents and RFCs. [A report](https://dnglbrstg7yg.cloudfront.net/08c33571ee8679775e810303f65c96c1d48e270d/compliance.html#/) is then generated, which makes it easy to track compliance with each requirement.
