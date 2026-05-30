// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark backend abstraction.
//!
//! All benchmark modules write against a single, criterion-compatible API.
//! The actual backend is selected at compile time via the `cachegrind` feature
//! flag:
//!
//! | feature | backend | measures |
//! |---------|---------|---------|
//! | *(none)* | [criterion](https://docs.rs/criterion) | wall-clock time |
//! | `cachegrind` | custom runner + callgrind client requests | instruction counts |
//!
//! # Querying the backend at runtime
//!
//! Benchmark code can branch on the active backend when needed:
//!
//! ```no_run
//! use s2n_quic_dc_benches::bench::{backend, Backend};
//!
//! match backend() {
//!     Backend::Criterion => { /* timing-sensitive setup */ }
//!     Backend::Cachegrind => { /* instruction-count-friendly setup */ }
//! }
//! ```
//!
//! # Running under Valgrind
//!
//! Build and run the `bench` binary under callgrind or cachegrind:
//!
//! ```sh
//! cargo build --bench bench --features cachegrind --profile bench
//!
//! # Callgrind: two output files per benchmark (<name> [cold] and <name> [warm])
//! valgrind --tool=callgrind \
//!     --callgrind-out-file=callgrind.%p \
//!     --instr-atstart=no \
//!     ./target/bench/deps/bench-*
//! callgrind_annotate callgrind.out.<pid>.*
//!
//! # Cachegrind: single output file for the whole run
//! valgrind --tool=cachegrind \
//!     --cachegrind-out-file=cachegrind.out \
//!     ./target/bench/deps/bench-*
//! cg_annotate cachegrind.out
//! ```

/// The benchmark backend currently in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// Wall-clock benchmarking via [criterion](https://docs.rs/criterion).
    Criterion,
    /// Instruction-count benchmarking for use with Valgrind
    /// (cachegrind or callgrind).
    Cachegrind,
}

/// Returns the active benchmark backend.
#[inline]
pub const fn backend() -> Backend {
    #[cfg(not(feature = "cachegrind"))]
    {
        Backend::Criterion
    }
    #[cfg(feature = "cachegrind")]
    {
        Backend::Cachegrind
    }
}

// ── Criterion backend (default) ───────────────────────────────────────────────

#[cfg(not(feature = "cachegrind"))]
pub use criterion::{BatchSize, BenchmarkGroup, BenchmarkId, Criterion, Throughput};
#[cfg(not(feature = "cachegrind"))]
pub use std::hint::black_box;

// ── Cachegrind backend ────────────────────────────────────────────────────────

#[cfg(feature = "cachegrind")]
pub use cachegrind_backend::{
    black_box, BatchSize, BenchmarkGroup, BenchmarkId, Criterion, Throughput,
};
#[cfg(feature = "cachegrind")]
mod cachegrind_backend {
    use core::fmt;
    use std::ffi::CString;
    pub use std::hint::black_box;

    // ── Throughput ────────────────────────────────────────────────────────────

    /// Throughput hint (kept for API parity; not used for measurement in
    /// cachegrind mode since we track instruction counts rather than data
    /// rates).
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum Throughput {
        Bytes(u64),
        BytesDecimal(u64),
        Elements(u64),
    }

    // ── BenchmarkId ───────────────────────────────────────────────────────────

    /// A benchmark identifier with a human-readable name and optional
    /// parameter, matching [criterion's `BenchmarkId`][crit] API.
    ///
    /// [crit]: https://docs.rs/criterion/latest/criterion/struct.BenchmarkId.html
    #[derive(Clone, Debug)]
    pub struct BenchmarkId {
        display: String,
    }

    impl BenchmarkId {
        /// Create a new identifier from a `name` and a `parameter`.
        ///
        /// Matches `criterion::BenchmarkId::new`.
        pub fn new<S: fmt::Display, P: fmt::Display>(id: S, parameter: P) -> Self {
            Self {
                display: format!("{id}/{parameter}"),
            }
        }
    }

    impl fmt::Display for BenchmarkId {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.display)
        }
    }

    // ── BatchSize ─────────────────────────────────────────────────────────────

    /// Batch-size hint kept for criterion API parity.
    #[derive(Clone, Copy, Debug)]
    #[non_exhaustive]
    pub enum BatchSize {
        SmallInput,
        LargeInput,
        PerIteration,
        NumBatches(u64),
        NumIterations(u64),
    }

    // ── Bencher ───────────────────────────────────────────────────────────────

    /// Single-iteration benchmark runner.
    ///
    /// In cachegrind mode each closure is executed **exactly twice** per
    /// [`Criterion::run_bench`] call — once for cold-cache and once for
    /// warm-cache measurements.  Running the resulting binary under
    /// `valgrind --tool=callgrind` (or `cachegrind`) then yields stable
    /// instruction counts and cache statistics for both scenarios.
    pub struct Bencher {
        cold_label: CString,
        warm_label: CString,
    }

    impl Bencher {
        #[inline(always)]
        fn new(cold_label: CString, warm_label: CString) -> Self {
            Self {
                cold_label,
                warm_label,
            }
        }

        /// Run `f` once.
        #[inline(always)]
        pub fn iter<F: FnMut()>(&mut self, mut f: F) {
            run_with_callgrind(
                || {
                    f();
                },
                self.cold_label.as_c_str(),
                self.warm_label.as_c_str(),
            );
        }

        /// Run setup outside instrumentation for both cold and warm benchmark
        /// passes, then run the measured routine once per pass.
        #[inline(always)]
        pub fn iter_batched<I, S, R>(&mut self, mut setup: S, mut routine: R, size: BatchSize)
        where
            S: FnMut() -> I,
            R: FnMut(I),
        {
            let _ = size;
            run_with_callgrind_setup(
                || setup(),
                |input| routine(input),
                self.cold_label.as_c_str(),
                self.warm_label.as_c_str(),
            );
        }

        /// Run `f` with a single iteration count and ignore the returned
        /// duration (cachegrind mode does not measure wall time).
        #[inline(always)]
        pub fn iter_custom<F>(&mut self, mut f: F)
        where
            F: FnMut(u64) -> std::time::Duration,
        {
            let _ = f(1);
        }
    }

    // ── BenchmarkGroup ────────────────────────────────────────────────────────

    /// A cachegrind-mode analogue of `criterion::BenchmarkGroup`.
    pub struct BenchmarkGroup<'c> {
        criterion: &'c mut Criterion,
        group_name: String,
    }

    impl<'c> BenchmarkGroup<'c> {
        /// Set throughput (no-op in cachegrind mode; kept for API parity).
        #[inline]
        pub fn throughput(&mut self, _throughput: Throughput) -> &mut Self {
            self
        }

        /// Register and immediately run a benchmark with a borrowed input.
        ///
        /// Matches `criterion::BenchmarkGroup::bench_with_input`.
        pub fn bench_with_input<I, F>(&mut self, id: BenchmarkId, input: &I, mut f: F) -> &mut Self
        where
            I: ?Sized,
            F: FnMut(&mut Bencher, &I),
        {
            let full_name = format!("{}/{id}", self.group_name);
            self.criterion.run_bench(&full_name, &mut |b| f(b, input));
            self
        }

        /// Register and immediately run a benchmark without input.
        ///
        /// Matches `criterion::BenchmarkGroup::bench_function`.
        pub fn bench_function<F>(&mut self, id: impl fmt::Display, mut f: F) -> &mut Self
        where
            F: FnMut(&mut Bencher),
        {
            let full_name = format!("{}/{id}", self.group_name);
            self.criterion.run_bench(&full_name, &mut f);
            self
        }

        /// Finish the group.  No-op in cachegrind mode; present for API
        /// parity.
        pub fn finish(self) {}
    }

    // ── Criterion ─────────────────────────────────────────────────────────────

    /// Top-level cachegrind benchmark runner.
    ///
    /// Create one instance, pass it to your `benchmarks` function, then let
    /// it drop (the `Drop` impl prints a summary).  Each call to
    /// [`BenchmarkGroup::bench_with_input`] / [`BenchmarkGroup::bench_function`]
    /// runs the closure **twice** — cold then warm — so a single Valgrind
    /// execution yields per-harness instruction counts for both scenarios.
    pub struct Criterion {
        count: usize,
    }

    impl Criterion {
        /// Create a new cachegrind runner.
        pub fn new() -> Self {
            Self { count: 0 }
        }

        /// Open a named benchmark group.
        ///
        /// Matches `criterion::Criterion::benchmark_group`.
        pub fn benchmark_group<S: Into<String>>(&mut self, name: S) -> BenchmarkGroup<'_> {
            BenchmarkGroup {
                criterion: self,
                group_name: name.into(),
            }
        }

        /// Run a single benchmark closure twice — cold then warm — emitting a
        /// labelled callgrind dump after each run.
        ///
        /// ## Cold run
        ///
        /// `callgrind::start_instrumentation()` flushes the simulated cache and
        /// starts instruction counting, so the first execution measures the
        /// cold-cache path.  After the run the stats are dumped and then zeroed
        /// so the warm run starts from a clean counter state.
        ///
        /// ## Warm run
        ///
        /// The second execution reuses the warm cache left by the cold run, so
        /// the dump reflects the steady-state (cache-hot) instruction counts.
        /// `callgrind::stop_instrumentation()` is called afterwards to exclude
        /// any inter-benchmark housekeeping from the next harness's cold run.
        ///
        /// Both `start_instrumentation` / `stop_instrumentation` and
        /// `dump_stats` are no-ops when not running under Valgrind, so the
        /// binary works correctly under cachegrind and without Valgrind too.
        fn run_bench(&mut self, name: &str, f: &mut dyn FnMut(&mut Bencher)) {
            self.count += 1;
            eprintln!("[cachegrind] {:>4}  {name}", self.count);

            let cold_label = make_label(name, "cold");
            let warm_label = make_label(name, "warm");
            let mut bencher = Bencher::new(cold_label, warm_label);
            f(&mut bencher);
        }
    }

    #[inline(always)]
    fn run_with_callgrind<R>(
        mut routine: R,
        cold_label: &std::ffi::CStr,
        warm_label: &std::ffi::CStr,
    )
    where
        R: FnMut(),
    {
        run_with_callgrind_setup(|| (), |_| routine(), cold_label, warm_label);
    }

    #[inline(always)]
    fn run_with_callgrind_setup<I, S, R>(
        mut setup: S,
        mut routine: R,
        cold_label: &std::ffi::CStr,
        warm_label: &std::ffi::CStr,
    ) where
        S: FnMut() -> I,
        R: FnMut(I),
    {
        // ── Cold run ──────────────────────────────────────────────────────────
        // Setup executes before instrumentation so setup work is excluded from
        // the measured instruction counts.
        let cold_input = setup();
        // start_instrumentation triggers a full simulated-cache flush so this
        // run captures cold-cache instruction counts.
        crabgrind::callgrind::start_instrumentation();
        routine(cold_input);
        crabgrind::callgrind::stop_instrumentation();
        crabgrind::callgrind::dump_stats(Some(cold_label));
        // Zero counters so the warm dump only reflects the second execution.
        crabgrind::callgrind::zero_stats();

        // ── Warm run ──────────────────────────────────────────────────────────
        // The cache is now populated from the cold run; this second execution
        // measures the steady-state (cache-hot) path. Setup is run again so the
        // warm pass benchmarks the same routine against a freshly prepared input.
        let warm_input = setup();
        crabgrind::callgrind::start_instrumentation();
        routine(warm_input);
        crabgrind::callgrind::stop_instrumentation();
        crabgrind::callgrind::dump_stats(Some(warm_label));
    }

    /// Build a `CString` label of the form `"<name> [<suffix>]"`.
    ///
    /// If `name` contains a null byte (which is disallowed in C strings) the
    /// name is silently truncated at that position and a warning is printed to
    /// stderr.
    fn make_label(name: &str, suffix: &str) -> CString {
        let display = format!("{name} [{suffix}]");
        CString::new(display.as_str()).unwrap_or_else(|e| {
            let pos = e.nul_position();
            eprintln!(
                "[cachegrind] warning: benchmark name {name:?} contains a null \
                 byte at position {pos}; dump label will be truncated"
            );
            CString::new(&display[..pos]).unwrap()
        })
    }

    impl Default for Criterion {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for Criterion {
        fn drop(&mut self) {
            if self.count > 0 {
                eprintln!(
                    "[cachegrind] completed {count} benchmarks",
                    count = self.count
                );
            }
        }
    }
}
