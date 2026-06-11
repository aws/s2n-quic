# Known Issue: Per-Endpoint Memory Leak in Simulation Teardown

## Summary

Each simulated endpoint pair (Client + Server) created inside a `crate::testing::sim(...)`
run retains roughly **55 MB** that is never reclaimed, even after the bach runtime for that
run is fully torn down. A single `sim()` call is harmless, but tests that call `sim()` in a
loop accumulate memory linearly and can exhaust system RAM.

This is **not** caused by the recv-credit flow-control work on the `reader-flow` branch, nor by
any of the credit-pool changes. It is a long-standing leak that predates all of that code.

## How it surfaces

The bolero fuzz tests in `endpoint/tests/deterministic.rs` —
`bulk_transfer_with_loss` and `transmission_rate_fuzz` — run thousands of `sim()` iterations
in a single process. At ~55 MB per iteration the process grows into the multi-GB range and is
SIGKILL'd (or, run unguarded across the full suite in parallel, can exhaust swap and take down
the machine). The fixed-pattern deterministic tests (`no_loss`, `initial_loss`,
`sporadic_loss`) each call `sim()` exactly once, so they stay near ~150 MB and pass.

## Evidence

Measured with a throwaway probe that runs 30 no-loss 256 KiB echo sims back to back and samples
the process RSS (current-process `ps -o rss=`). The growth is linear at ~55 MB per `sim()`:

```text
after 10 sims  ~  570 MB
after 20 sims  ~ 1130 MB
after 30 sims  ~ 1675 MB
```

Two controls localize it to per-endpoint allocations rather than the sim runtime or the test
registries:

- An **empty** sim body (`sim(|| {})`, no endpoint, no transfer) leaks **0 MB/run** — RSS is
  flat across 30 iterations. So it is not bach runtime state.
- Clearing the `SIM_MAP_REGISTRY` / `SIM_ADDR_REGISTRY` / `SIM_PARAMS_REGISTRY` /
  `SIM_ENDPOINT_BY_GROUP` thread-locals between runs changes nothing. So it is not those
  registries holding the retained `PathSecretMap` clones.

## Bisection — predates all relevant work

The same probe shows identical ~55 MB/run growth at every point checked:

| Commit       | What it contains                              | Leak/run |
| ------------ | --------------------------------------------- | -------- |
| `c6b355b5`   | before any credit-pool code exists            | ~54 MB   |
| `88747532`   | branch point: credit pool + writer wiring     | ~56 MB   |
| `aa0fbbb4`   | `reader-flow` HEAD (full recv-flow work)       | ~56 MB   |

Because it reproduces before the credit pool was introduced, the leak is unrelated to credit or
recv-flow-control and lives in the shared endpoint/sim setup or teardown path.

## Where to look

The retained memory survives a full bach-runtime teardown, so something holds a **strong**
reference to the `Endpoint` (and transitively its ~50,000-entry `PathSecretMap`, descriptor
pools, and socket buffers) past the end of the sim. `SIM_ENDPOINT_BY_GROUP` only holds a `Weak`,
so in principle the endpoint should drop when the test's `Client`/`Server` handles drop inside
the sim — but it evidently does not. Likely candidates:

- A background worker thread spawned during endpoint setup that outlives the sim and keeps an
  `Arc<Endpoint>` (or an `Arc` to one of its subsystems) alive.
- A global/static collection that an endpoint subsystem registers itself into and never removes.
- An `Arc` reference cycle among endpoint tasks/channels that prevents the strong count from
  reaching zero.

A good next step is to wrap the endpoint `Arc` in the sim harness, record
`Arc::strong_count` immediately after the sim block returns, and confirm it is `> 0`; then walk
the subsystems that captured a clone.

## Reproducing

Add a throwaway `#[ignore]` test next to the deterministic tests that loops `sim()` and prints
RSS (see the probe described above), then run it with a hard memory cap so a regression cannot
take down the machine:

```sh
cargo nextest run -p s2n-quic-dc --run-ignored all -E 'test(<probe_name>)' --no-capture
```

Until the leak is fixed, run the bolero fuzz tests (`bulk_transfer_with_loss`,
`transmission_rate_fuzz`) under an external memory watchdog rather than unbounded.
