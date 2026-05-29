// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Per-peer queue slots for routing inbound `msg::Stream` and `msg::Control`
//! frames to the correct stream handler.
//!
//! Each peer connection owns a flat array of [`slot::Slot`]s held in a pinned
//! [`page_table::PageTable`].  A slot is identified by its index, which is
//! exchanged in the QUIC handshake as a `queue_id`.
//!
//! ## Roles
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`half`] | Single queue half with push/pop/poll and open/close lifecycle |
//! | [`slot`] | One queue slot (two halves + atomic `binding_id`) |
//! | [`page_table`] | Pinned page-table; stable slot addresses, O(1) dispatch |
//! | [`freed`] | Server freed-queue accumulator and batch emission |
//! | [`handle`] | `StreamReceiver`, `ControlReceiver`, `AllocResult` |
//! | [`client`] | `ClientAllocator` + `ClientDispatch` + `ClientFreeList` |
//! | [`server`] | `ServerDispatch` + `BindResult` |

pub(crate) mod client;
pub(crate) mod freed;
pub(crate) mod half;
pub(crate) mod handle;
pub(crate) mod msg_entry;
pub(crate) mod msg_table;
pub(crate) mod page_table;
pub(crate) mod server;
pub(crate) mod slot;

#[cfg(test)]
pub(crate) mod testing;

// Public API surface

pub use half::{AutoWake, Closed};
pub use handle::{AllocResult, ControlReceiver, StreamReceiver};
pub use server::BindResult;

pub use client::{ClientAllocFuture, ClientDispatch, ClientState};
pub use freed::{freed_batch_channel, FreedBatchRx, FreedBatchTx, FreedInner};
pub use server::{ServerState, ServerView};

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned by dispatch operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<T> {
    /// No slot is allocated at this `queue_id`.
    Unallocated(T),
    /// The slot is allocated but this receiver half has been dropped.
    HalfClosed(T),
    /// The sender was closed (path secret evicted).
    SenderClosed,
    /// The binding_id refers to an old (already-recycled) generation of this slot.
    StaleBinding(T),
    /// The binding_id is ahead of the current slot binding.  This indicates a
    /// bug — the slot hasn't been freed yet but a future binding arrived.
    FutureBinding(T),
    /// The queue_id exceeds the negotiated cap.  Protocol violation.
    CapExceeded(T),
}

/// Error returned by QueueMsg dispatch operations.
#[derive(Debug)]
pub enum MsgError<E> {
    /// Queue validation failed before the write callback could complete.
    Queue(Error<()>),
    /// The caller-provided write callback failed.
    Write(E),
}

impl<T> From<half::Error<T>> for Error<T> {
    fn from(e: half::Error<T>) -> Self {
        match e {
            half::Error::Unallocated(t) => Error::Unallocated(t),
            half::Error::HalfClosed(t) => Error::HalfClosed(t),
            half::Error::SenderClosed => Error::SenderClosed,
        }
    }
}
