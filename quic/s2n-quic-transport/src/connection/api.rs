// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines the public QUIC connection API

use crate::{
    connection::{self, ConnectionApi},
    stream::{ops, Stream, StreamError, StreamId},
};
use bytes::Bytes;
use core::{
    fmt,
    num::NonZeroU64,
    sync::atomic::{self, Ordering},
    task::{Context, Poll},
};
use s2n_quic_core::{
    application,
    application::ServerName,
    event::query::{Query, QueryMut},
    inet::SocketAddress,
    stream::StreamType,
};

/// A QUIC connection
pub struct Connection {
    /// The inner connection API implementation
    ///
    /// This uses a dynamically-dispatched interface to hide all of the connection's
    /// generic parameters and allows applications to interact with connections in a
    /// straightforward manner.
    pub(super) api: ConnectionApi,

    /// The open token associated with this each connection handle.
    ///
    /// This is used to correctly track `poll_open_stream` requests.
    open_token: OpenToken,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO print interesting virtual fields
        f.debug_struct("Connection").finish()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        debug_assert!(
            self.api.application_handle_count().load(Ordering::Acquire) > 0,
            "application_handle_count underflowed"
        );

        // Safety
        //
        // The use of Ordering and fence mirrors the `Arc` implementation in
        // the standard library.
        //
        // This fence is needed to prevent reordering of use of the data and
        // deletion of the data.  Because it is marked `Release`, the decreasing
        // of the reference count synchronizes with this `Acquire` fence. This
        // means that use of the data happens before decreasing the reference
        // count, which happens before this fence, which happens before the
        // deletion of the data.
        // https://github.com/rust-lang/rust/blob/e012a191d768adeda1ee36a99ef8b92d51920154/library/alloc/src/sync.rs#L1637

        // If the connection wasn't closed before, close it now to make sure
        // all Streams terminate.
        //
        // Only close the connection if this is the last application handle.
        // Otherwise, just drop `api`, which decrements the strong count.
        if self
            .api
            .application_handle_count()
            .fetch_sub(1, Ordering::Release)
            != 1
        {
            return;
        }

        atomic::fence(Ordering::Acquire);
        self.api.close_connection(None);
    }
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        // Safety
        //
        // Using a relaxed ordering is alright here, as knowledge of the
        // original reference prevents other threads from erroneously deleting
        // the object.
        // https://github.com/rust-lang/rust/blob/e012a191d768adeda1ee36a99ef8b92d51920154/library/alloc/src/sync.rs#L1329
        self.api
            .application_handle_count()
            .fetch_add(1, Ordering::Relaxed);
        Self {
            api: self.api.clone(),
            // don't clone the open token - each instance should have its own token
            open_token: OpenToken::new(),
        }
    }
}

impl Connection {
    pub(crate) fn new(api: ConnectionApi) -> Self {
        // Safety
        //
        // Using a relaxed ordering is alright here, as knowledge of the
        // original reference prevents other threads from erroneously deleting
        // the object.
        // https://github.com/rust-lang/rust/blob/e012a191d768adeda1ee36a99ef8b92d51920154/library/alloc/src/sync.rs#L1329
        api.application_handle_count()
            .fetch_add(1, Ordering::Relaxed);
        Self {
            api,
            open_token: OpenToken::new(),
        }
    }

    /// Accepts an incoming [`Stream`]
    ///
    /// The method will return
    /// - `Poll::Ready(Ok(Some(stream, stream_type)))` if a [`Stream`] was accepted
    /// - `Poll::Ready(Ok(None))` if the connection was closed without an error
    /// - `Poll::Ready(Err(stream_error))` if no stream could be accepted due to an error
    /// - `Poll::Pending` if no new [`Stream`] of the given type was accepted by the connection yet.
    ///   In this case the caller must retry calling [`Self::poll_accept`].
    ///   For this purpose the method will save the [`core::task::Waker`]
    ///   which is provided as part of the [`Context`] parameter, and notify it
    ///   as soon as retrying the method will yield a different result.
    #[inline]
    pub fn poll_accept(
        &mut self,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<Stream>, connection::Error>> {
        self.api.poll_accept(&self.api, stream_type, context)
    }

    #[inline]
    pub fn poll_open_stream(
        &mut self,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<Result<Stream, connection::Error>> {
        self.api
            .poll_open_stream(&self.api, stream_type, &mut self.open_token, context)
    }

    #[inline]
    pub fn poll_request(
        &self,
        stream_id: StreamId,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        self.api.poll_request(stream_id, request, context)
    }

    /// Closes the Connection with the provided error code
    ///
    /// This will immediately terminate all outstanding streams.
    #[inline]
    pub fn close(&self, error_code: application::Error) {
        self.api.close_connection(Some(error_code));
    }

    #[inline]
    pub fn server_name(&self) -> Result<Option<ServerName>, connection::Error> {
        self.api.server_name()
    }

    #[inline]
    pub fn application_protocol(&self) -> Result<Bytes, connection::Error> {
        self.api.application_protocol()
    }

    #[inline]
    pub fn id(&self) -> u64 {
        self.api.id()
    }

    #[inline]
    pub fn ping(&self) -> Result<(), connection::Error> {
        self.api.ping()
    }

    pub fn keep_alive(&self, enabled: bool) -> Result<(), connection::Error> {
        self.api.keep_alive(enabled)
    }

    #[inline]
    pub fn local_address(&self) -> Result<SocketAddress, connection::Error> {
        self.api.local_address()
    }

    #[inline]
    pub fn remote_address(&self) -> Result<SocketAddress, connection::Error> {
        self.api.remote_address()
    }

    #[inline]
    pub fn query_event_context(&self, query: &mut dyn Query) -> Result<(), connection::Error> {
        self.api.query_event_context(query)
    }

    #[inline]
    pub fn query_event_context_mut(
        &self,
        query: &mut dyn QueryMut,
    ) -> Result<(), connection::Error> {
        self.api.query_event_context_mut(query)
    }
}

/// An opaque token issued to each connection handle which allows the stream
/// controller to track any pending open requests.
///
/// Each connection handle must have a unique instance of a token so their wakers are correctly
/// tracked.
#[derive(Debug)]
pub struct OpenToken(Option<NonZeroU64>);

impl OpenToken {
    /// Creates a new open token
    ///
    /// This should be held on the connection handle and should be presented each time
    /// `poll_open_stream` is called.
    #[inline]
    pub const fn new() -> Self {
        Self(None)
    }

    /// Creates an open token counter, used for issuing tokens to callers
    #[inline]
    pub(crate) fn counter() -> Self {
        Self(Some(unsafe {
            // Safety: 1 is always non-zero
            NonZeroU64::new_unchecked(1)
        }))
    }

    /// Returns the next open token for a connection
    #[inline]
    pub(crate) fn next(&mut self) -> Self {
        let v = self.0.expect("token counter should always be initialized");
        let next = Self(Some(v));
        self.0 = Some(unsafe {
            // Safety: N + 1 is always non-zero
            NonZeroU64::new_unchecked(v.get() + 1)
        });
        next
    }

    /// Returns the index that the caller's waker should be stored, given the expired token
    #[inline]
    pub(crate) fn index(&self, expired_token: &Self) -> Option<usize> {
        let base = expired_token.0.map_or(0, |v| v.get());
        // since we're dealing with NonZeroU64, add one to get the correct index
        let base = base + 1;
        let v = self.0?;
        let v = v.get().checked_sub(base)?;
        Some(v as usize)
    }

    /// Expires a `count` number of tokens
    ///
    /// This should be called each time at least one waker is woken and removed from the waker list
    #[inline]
    pub(crate) fn expire(&mut self, count: usize) {
        if let Some(v) = self.0.as_mut() {
            *v = unsafe {
                // Safety: non-zero N + count is always non-zero
                NonZeroU64::new_unchecked(v.get() + count as u64)
            };
        } else if let Some(v) = NonZeroU64::new(count as _) {
            *self = Self(Some(v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_token_test() {
        let mut caller_tokens = [OpenToken::new(), OpenToken::new(), OpenToken::new()];
        let mut expired_token = OpenToken::new();
        let mut token_counter = OpenToken::counter();

        assert_eq!(
            caller_tokens[0].index(&expired_token),
            None,
            "empty caller token with no expired tokens"
        );

        // give the caller a token
        caller_tokens[0] = token_counter.next();
        assert_eq!(
            caller_tokens[0].index(&expired_token),
            Some(0),
            "issued caller with no expired tokens should be index 0"
        );

        // expire the issued token
        expired_token.expire(1);
        assert_eq!(
            caller_tokens[0].index(&expired_token),
            None,
            "issued caller token should now be expired"
        );

        for (idx, token) in caller_tokens.iter_mut().enumerate() {
            *token = token_counter.next();
            assert_eq!(
                token.index(&expired_token),
                Some(idx),
                "issued caller should be indexed in order",
            );
        }

        // expire just the first token
        expired_token.expire(1);

        assert_eq!(
            caller_tokens[0].index(&expired_token),
            None,
            "first caller token should now be expired"
        );

        for (idx, token) in caller_tokens[1..].iter().enumerate() {
            assert_eq!(
                token.index(&expired_token),
                Some(idx),
                "caller {} should not be expired",
                idx + 1,
            );
        }
    }
}
