// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Flow-based queue allocation and dispatching.
//!
//! This provides queue infrastructure similar to `stream::recv::dispatch` but
//! is generic over the stream and control data types.

use crate::{credentials::Credentials, intrusive_queue};
use s2n_quic_core::varint::VarInt;

mod descriptor;
mod free_list;
mod handle;
mod inner;
mod pool;
mod probes;
mod sender;

// Re-export the Key trait
pub use descriptor::{Key, ValidationError};
pub use inner::AutoWake;

/// Allocate this many channels at a time
///
/// With `debug_assertions`, we allocate smaller pages to try and cover more
/// branches in the allocator logic around growth.
const PAGE_SIZE: usize = if cfg!(debug_assertions) {
    8
} else {
    u16::MAX as _
};

pub type Error<T> = inner::Error<T>;
pub type Control<S, C, K> = handle::Control<S, C, K>;
pub type Stream<S, C, K> = handle::Stream<S, C, K>;

/// Queue allocator for flow-based routing
///
/// Generic over stream data type `S` and control data type `C`.
pub struct Allocator<S, C, K = Credentials>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    K: 'static + Send + Sync,
{
    pool: pool::Pool<S, C, K, PAGE_SIZE>,
}

impl<S, C, K> Clone for Allocator<S, C, K>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    K: 'static + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

impl<S, C, K> Allocator<S, C, K>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    K: 'static + Send + Sync,
{
    pub fn new() -> Self {
        Self {
            pool: pool::Pool::new(),
        }
    }

    #[inline]
    pub fn dispatcher(&self) -> Dispatch<S, C, K> {
        Dispatch {
            senders: self.pool.senders(),
            is_open: true,
            pool: self.pool.clone(),
        }
    }

    #[inline]
    pub fn alloc(
        &self,
        key: K,
        remote_queue_id: Option<VarInt>,
    ) -> Result<(Control<S, C, K>, Stream<S, C, K>), K> {
        self.pool.alloc(key, remote_queue_id)
    }

    #[inline]
    pub fn alloc_or_grow(
        &mut self,
        key: K,
        remote_queue_id: Option<VarInt>,
    ) -> (Control<S, C, K>, Stream<S, C, K>) {
        self.pool.alloc_or_grow(key, remote_queue_id)
    }
}

/// Dispatcher which routes data to the specified queue
pub struct Dispatch<S, C, K = Credentials>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    K: 'static + Send + Sync,
{
    senders: sender::Senders<S, C, K, PAGE_SIZE>,
    is_open: bool,
    pool: pool::Pool<S, C, K, PAGE_SIZE>,
}

impl<S, C, K> Clone for Dispatch<S, C, K>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    K: 'static + Send + Sync + Key,
{
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
            is_open: self.is_open,
            pool: self.pool.clone(),
        }
    }
}

impl<S, C, K> Dispatch<S, C, K>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    K: 'static + Send + Sync + Key,
{
    #[inline]
    pub fn alloc(
        &self,
        key: K,
        remote_queue_id: Option<VarInt>,
    ) -> Result<(Control<S, C, K>, Stream<S, C, K>), K> {
        self.pool.alloc(key, remote_queue_id)
    }

    #[inline]
    pub fn alloc_or_grow(
        &mut self,
        key: K,
        remote_queue_id: Option<VarInt>,
    ) -> (Control<S, C, K>, Stream<S, C, K>) {
        self.pool.alloc_or_grow(key, remote_queue_id)
    }

    #[inline]
    pub fn send_control(
        &mut self,
        local_queue_id: VarInt,
        remote_queue_id: Option<VarInt>,
        params: &K::Request,
        data: intrusive_queue::Entry<C>,
    ) -> Result<AutoWake, Error<intrusive_queue::Entry<C>>> {
        let res = self.senders.lookup(local_queue_id, data, |sender, data| {
            sender.send_control(data, remote_queue_id, params)
        });

        match res {
            Ok(waker) => {
                tracing::trace!(%local_queue_id, "send_control");
                Ok(waker)
            }
            Err(Error::PermanentlyClosed) => {
                self.is_open = false;
                Err(inner::Error::PermanentlyClosed)
            }
            Err(Error::HalfClosed(data)) => {
                tracing::debug!(%local_queue_id, "control receiver closed");
                Err(inner::Error::HalfClosed(data))
            }
            Err(Error::ValidationFailed(data, reason)) => {
                tracing::debug!(%local_queue_id, ?reason, "control queue validation failed");
                Err(inner::Error::ValidationFailed(data, reason))
            }
            Err(Error::Unallocated(data)) => {
                tracing::debug!("unroutable control data");
                Err(inner::Error::Unallocated(data))
            }
        }
    }

    #[inline]
    pub fn send_stream(
        &mut self,
        local_queue_id: VarInt,
        remote_queue_id: Option<VarInt>,
        params: &K::Request,
        data: intrusive_queue::Entry<S>,
    ) -> Result<AutoWake, Error<intrusive_queue::Entry<S>>> {
        let res = self.senders.lookup(local_queue_id, data, |sender, data| {
            sender.send_stream(data, remote_queue_id, params)
        });

        match res {
            Ok(waker) => {
                tracing::trace!(%local_queue_id, "send_stream");
                Ok(waker)
            }
            Err(Error::PermanentlyClosed) => {
                self.is_open = false;
                Err(inner::Error::PermanentlyClosed)
            }
            Err(Error::HalfClosed(data)) => {
                tracing::debug!(%local_queue_id, "stream receiver closed");
                Err(inner::Error::HalfClosed(data))
            }
            Err(Error::ValidationFailed(data, reason)) => {
                tracing::debug!(%local_queue_id, ?reason, "stream queue validation failed");
                Err(inner::Error::ValidationFailed(data, reason))
            }
            Err(Error::Unallocated(data)) => {
                tracing::debug!("unroutable stream data");
                Err(inner::Error::Unallocated(data))
            }
        }
    }

    #[inline]
    pub fn send_both(
        &mut self,
        local_queue_id: VarInt,
        remote_queue_id: Option<VarInt>,
        params: &K::Request,
        stream_data: intrusive_queue::Entry<S>,
        control_data: intrusive_queue::Entry<C>,
    ) -> (AutoWake, AutoWake) {
        let res = self.senders.lookup(local_queue_id, (), |sender, ()| {
            let send = sender
                .send_stream(stream_data, remote_queue_id, params)
                .unwrap_or_default();
            let recv = sender
                .send_control(control_data, remote_queue_id, params)
                .unwrap_or_default();

            Ok((send, recv))
        });

        tracing::trace!(%local_queue_id, "send_both");

        res.unwrap_or_default()
    }

    /// Validates the queue's key against the provided parameters by checking the stream queue.
    #[inline]
    pub fn validate_stream(
        &mut self,
        local_queue_id: VarInt,
        params: &K::Request,
    ) -> Result<(), ValidateError> {
        match self.senders.lookup(local_queue_id, (), |sender, ()| {
            Ok(sender.validate_stream(params))
        }) {
            Ok(result) => result,
            Err(_) => Err(ValidateError::Unallocated),
        }
    }

    /// Validates the queue's key against the provided parameters by checking the control queue.
    #[inline]
    pub fn validate_control(
        &mut self,
        queue_id: VarInt,
        params: &K::Request,
    ) -> Result<(), ValidateError> {
        match self.senders.lookup(queue_id, (), |sender, ()| {
            Ok(sender.validate_control(params))
        }) {
            Ok(result) => result,
            Err(_) => Err(ValidateError::Unallocated),
        }
    }
}

/// Error returned by validate_stream/validate_control
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidateError {
    /// Queue not found or deallocated
    Unallocated,
    /// Key validation failed
    Validation(ValidationError),
}
