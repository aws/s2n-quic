// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract-test style guide for `endpoint::tasks`.
//!
//! ## Structure
//!
//! Each test spawns at least two interacting tasks:
//!
//! 1. A **driver/feeder** task that puts the pipeline into the state under test.
//! 2. A **pipeline** task running the function under test.
//! 3. An **assertion** task (or the same driver task after `drain_budgeted`) that checks
//!    every output channel.
//!
//! ## Assertion patterns
//!
//! **Assert on every output channel** — both those you expect to fire AND those you expect
//! to stay silent.  Silent channels are just as important as active ones; skipping them
//! silently allows regressions in routing logic.
//!
//! ```text
//! assert!(tx_wheel_rx.recv().await.is_some(), "TX wheel should be re-armed");
//! assert!(pto_wheel_rx.recv().await.is_none(), "PTO wheel must not fire");
//! ```
//!
//! **Use real channels everywhere** — never use a no-op sink for output channels.  Every
//! output is an opportunity to assert something.
//!
//! ## Triggering interesting states
//!
//! - **TX wheel re-arm**: push enough pending frames that not all fit in one assembly pass
//!   (e.g. two frames whose combined encoded size exceeds the MTU).  After assembly the
//!   first frame is sent; the second remains pending and `wheel_interest` re-arms the TX
//!   wheel.
//!
//! - **Cancelled frame on `cancelled` channel**: create a `frame::completion_channel()`,
//!   call `rx.cancel()` before pushing the frame.  The assembler sees
//!   `!frame.should_transmit()` and routes it to `cancelled`.
//!
//! - **`ack_completions` channel**: push an `msg::Sender::PendingAck` submission into
//!   `context.pending_acks` before assembly.
//!
//! ## Bach simulation patterns
//!
//! - Use `.group("server")` on the receiving task to register a named group, then resolve
//!   its simulated IP with `test_entry_at("server:4433").await` in the feeder task.
//!
//! - `drain_budgeted` takes `self` (consuming the pipeline receiver), which drops all
//!   internal senders.  Empty output receivers then return `None` immediately, so
//!   assertions can follow the drain without extra synchronisation.
//!
//! ## Reducing boilerplate
//!
//! Use `helpers::build_send_context` to create a `send::Context` wrapped in
//! `Rc<RefCell<_>>`, and `helpers::test_batch_with_payload` to create frames large enough
//! to overflow a single MTU pass (forcing TX-wheel re-arm in multi-frame tests).

mod ack_burst;
mod ack_completion;
mod completion;
mod context_resolver;
mod frame_dispatch;
mod helpers;
mod idle_wheel;
mod invalidation;
mod invalidation_validator;
mod send_socket_assembler;
mod send_worker;
mod socket_recv;
mod waker_drain;
