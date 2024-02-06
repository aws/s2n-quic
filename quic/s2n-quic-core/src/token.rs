// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    event::{api::SocketAddress, IntoEvent},
    inet, random,
};

#[non_exhaustive]
pub struct Context<'a> {
    pub remote_address: SocketAddress<'a>,
    pub peer_connection_id: &'a [u8],
    pub random: &'a mut dyn random::Generator,
}

impl<'a> Context<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(
        remote_address: &'a inet::SocketAddress,
        peer_connection_id: &'a connection::PeerId,
        random: &'a mut dyn random::Generator,
    ) -> Self {
        Self {
            remote_address: remote_address.into_event(),
            peer_connection_id: peer_connection_id.as_bytes(),
            random,
        }
    }
}

pub trait Format: 'static + Send {
    const TOKEN_LEN: usize;

    /// Generate a signed token to be delivered in a NEW_TOKEN frame.
    /// This function will only be called if the provider support NEW_TOKEN frames.
    fn generate_new_token(
        &mut self,
        context: &mut Context<'_>,
        source_connection_id: &connection::LocalId,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Generate a signed token to be delivered in a Retry Packet
    fn generate_retry_token(
        &mut self,
        context: &mut Context<'_>,
        original_destination_connection_id: &connection::InitialId,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Return the original destination connection id of a valid token.
    /// If the token is invalid, return None.
    /// Callers should detect duplicate tokens and treat them as invalid.
    fn validate_token(
        &mut self,
        context: &mut Context<'_>,
        token: &[u8],
    ) -> Option<connection::InitialId>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    RetryPacket,
    NewTokenFrame,
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::crypto::retry;

    #[derive(Debug, Default)]
    pub struct Format(());

    impl super::Format for Format {
        const TOKEN_LEN: usize = retry::example::TOKEN_LEN;

        fn generate_new_token(
            &mut self,
            _context: &mut Context<'_>,
            _source_connection_id: &connection::LocalId,
            _output_buffer: &mut [u8],
        ) -> Option<()> {
            // TODO implement one for testing
            None
        }

        fn generate_retry_token(
            &mut self,
            _context: &mut Context<'_>,
            _original_destination_connection_id: &connection::InitialId,
            output_buffer: &mut [u8],
        ) -> Option<()> {
            output_buffer.copy_from_slice(&retry::example::TOKEN);
            Some(())
        }

        fn validate_token(
            &mut self,
            _context: &mut Context<'_>,
            token: &[u8],
        ) -> Option<connection::InitialId> {
            if token == retry::example::TOKEN {
                return Some(connection::InitialId::TEST_ID);
            }

            None
        }
    }

    impl Format {
        pub fn new() -> Self {
            Self(())
        }
    }
}
